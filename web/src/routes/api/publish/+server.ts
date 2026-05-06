import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import {
	validateSecurity,
	hasErrors,
	sanitizeErrorMessage,
	normalizeAiWarnings
} from '$lib/server/security.js';
import { explainPattern } from '$lib/server/security-explanations.js';
import { fetchGithubFile } from '$lib/server/github.js';
import { db, schema } from '$lib/server/db.js';
import { eq, sql } from 'drizzle-orm';

const { userPipelines } = schema;

// POST /api/publish — Fetch code from GitHub, validate security, store URL + metadata
export const POST: RequestHandler = async ({ request }) => {
	try {
		const body = await request.json();
		const { github_url, github_token, forked_from, ai_warnings } = body;
		// AI-detected suspicious patterns the publisher already approved
		// in the MCP client. Stored alongside the pipeline so downloaders
		// can review the same list before fetching files.
		const approvedWarnings = normalizeAiWarnings(ai_warnings);

		if (!github_url || !github_token) {
			return json({ error: 'github_url and github_token are required' }, { status: 400 });
		}

		// Validate that github_url actually points to GitHub (SSRF prevention)
		if (!/^https:\/\/github\.com\/[^/]+\/[^/]+/.test(github_url)) {
			return json({ error: 'github_url must be a valid GitHub repository URL' }, { status: 400 });
		}

		// 1. Validate GitHub token and get username for author
		const userResp = await fetch('https://api.github.com/user', {
			headers: {
				Authorization: `Bearer ${github_token}`,
				'User-Agent': 'autopipe-registry'
			}
		});
		if (!userResp.ok) {
			return json({ error: 'Invalid GitHub token' }, { status: 401 });
		}
		const githubUser = await userResp.json();
		const author = githubUser.login as string;

		// 2. Fetch required files from GitHub for validation. Bypass the
		// 5-minute file cache so a re-publish that follows a fix-and-push
		// loop sees the fresh content instead of last attempt's snapshot.
		let snakefile: string, dockerfile: string, metadata_json: string;
		try {
			[snakefile, dockerfile, metadata_json] = await Promise.all([
				fetchGithubFile(github_url, 'Snakefile', undefined, true),
				fetchGithubFile(github_url, 'Dockerfile', undefined, true),
				fetchGithubFile(github_url, 'ro-crate-metadata.json', undefined, true)
			]);
		} catch {
			return json({ error: 'Failed to fetch files from GitHub repository' }, { status: 400 });
		}

		// 3. Check required files
		if (!snakefile || !dockerfile) {
			return json(
				{ error: 'GitHub repository must contain Snakefile and Dockerfile' },
				{ status: 400 }
			);
		}

		// 4. Parse ro-crate-metadata.json (supports RO-Crate format)
		let metadata: Record<string, unknown>;
		try {
			const raw = metadata_json ? JSON.parse(metadata_json) : {};
			// Check if RO-Crate format
			if (raw['@context'] && raw['@graph']) {
				const graph = raw['@graph'] as Array<Record<string, unknown>>;
				const dataset = graph.find((n: Record<string, unknown>) => n['@id'] === './');
				if (!dataset) {
					return json({ error: 'ro-crate-metadata.json missing Dataset node (@id: "./")' }, { status: 400 });
				}
				// Extract fields from RO-Crate Dataset node
				const tools = ((dataset['softwareRequirements'] as Array<{['@id']: string}>) || [])
					.map(ref => {
						const node = graph.find((n: Record<string, unknown>) => n['@id'] === ref['@id']);
						return node ? (node['name'] as string) : '';
					}).filter(Boolean);
				const input_formats = ((dataset['input'] as Array<{['@id']: string}>) || [])
					.map(ref => {
						const node = graph.find((n: Record<string, unknown>) => n['@id'] === ref['@id']);
						return node ? (node['name'] as string) : '';
					}).filter(Boolean);
				const output_formats = ((dataset['output'] as Array<{['@id']: string}>) || [])
					.map(ref => {
						const node = graph.find((n: Record<string, unknown>) => n['@id'] === ref['@id']);
						return node ? (node['name'] as string) : '';
					}).filter(Boolean);
				const creator_refs = (dataset['creator'] as Array<{['@id']: string}>) || [];
				const author_name = creator_refs.length > 0
					? (graph.find((n: Record<string, unknown>) => n['@id'] === creator_refs[0]['@id'])?.['name'] as string || '')
					: '';
				// Extract isBasedOn URL (e.g., WorkflowHub source)
				const isBasedOn = dataset['isBasedOn'] as { '@id'?: string } | string | undefined;
				const based_on_url = typeof isBasedOn === 'string'
					? isBasedOn
					: (isBasedOn?.['@id'] || null);

				metadata = {
					name: dataset['name'] as string,
					description: (dataset['description'] as string) || '',
					version: (dataset['version'] as string) || '1.0.0',
					author: author_name,
					tools,
					input_formats,
					output_formats,
					tags: (dataset['keywords'] as string[]) || [],
					verified: false,
					based_on_url: based_on_url && based_on_url.length > 0 ? based_on_url : null
				};
			} else {
				metadata = raw;
			}
		} catch {
			return json({ error: 'ro-crate-metadata.json is not valid JSON' }, { status: 400 });
		}

		if (!metadata.name) {
			return json({ error: 'ro-crate-metadata.json must contain a "name" field' }, { status: 400 });
		}

		// 5. Hard-layer security validation — only critical patterns on the
		// Snakefile/Dockerfile. Soft (warning-level) review is delegated to
		// the MCP client's AI, whose results arrive via `ai_warnings`.
		const issues = validateSecurity(snakefile, dockerfile);
		if (hasErrors(issues)) {
			const enriched = issues.map((i) => {
				const ex = explainPattern(i.pattern_id);
				return { ...i, short: ex.short, detail: ex.detail };
			});
			return json(
				{
					error: 'Security validation failed',
					issues: enriched,
					hint: 'Each issue lists the file, line, pattern explanation, and reason. Fix the code and republish.'
				},
				{ status: 422 }
			);
		}

		// 6. Always INSERT a new record (version tracking)
		let name = metadata.name as string;
		let version = (metadata.version as string) || '1.0.0';

		// Auto-derive forked_from from based_on_url when it points to our own
		// Hub. download_pipeline injects an isBasedOn entry into the ro-crate
		// pointing to the source Hub URL, so users get lineage tracking even
		// when they rename or modify the pipeline before publishing.
		let autoForkedFrom: number | null = null;
		if (metadata.based_on_url && typeof metadata.based_on_url === 'string') {
			const url = metadata.based_on_url as string;
			const own = (process.env.PUBLIC_HUB_URL || 'https://hub.autopipe.org').replace(/\/+$/, '');
			if (url.startsWith(own + '/pipelines/')) {
				const m = url.match(/\/pipelines\/(\d+)(?:\/|$|\?)/);
				if (m) {
					const parsed = parseInt(m[1], 10);
					if (!Number.isNaN(parsed)) autoForkedFrom = parsed;
				}
			}
		}

		// Explicit forked_from from caller wins over auto-detection. Pass
		// forked_from = null explicitly to break the lineage suggested by
		// based_on_url.
		const resolvedForkedFrom: number | null =
			typeof forked_from === 'number' ? forked_from : autoForkedFrom;

		if (resolvedForkedFrom !== null) {
			// forked_from specified → record lineage to the parent pipeline.
			// Use the name from ro-crate-metadata.json as-is. We do NOT overwrite
			// it with the parent's name even when the author matches: the user's
			// rename intent (different name in ro-crate) must be respected, and
			// silently overwriting causes duplicate registrations under the
			// wrong name.
			const [parent] = await db
				.select({ author: userPipelines.author, name: userPipelines.name })
				.from(userPipelines)
				.where(eq(userPipelines.pipelineId, resolvedForkedFrom))
				.limit(1);

			if (parent && parent.author !== author) {
				// Different author → fork: independent version chain starting at 1.0.0
				version = '1.0.0';
			}
			// else: same author keeps whatever version the metadata declared.
		} else {
			// No forked_from → name deduplication if same name exists
			const existing = await db
				.select({ pipelineId: userPipelines.pipelineId })
				.from(userPipelines)
				.where(eq(userPipelines.name, name))
				.limit(1);
			if (existing.length > 0) {
				let suffix = 2;
				const MAX_DEDUP_ATTEMPTS = 100;
				while (suffix <= MAX_DEDUP_ATTEMPTS) {
					const candidate = `${metadata.name} ${suffix}`;
					const dup = await db
						.select({ pipelineId: userPipelines.pipelineId })
						.from(userPipelines)
						.where(eq(userPipelines.name, candidate))
						.limit(1);
					if (dup.length === 0) {
						name = candidate;
						break;
					}
					suffix++;
				}
				if (suffix > MAX_DEDUP_ATTEMPTS) {
					return json({ error: 'Too many pipelines with this name' }, { status: 409 });
				}
			}
		}

		const [row] = await db
			.insert(userPipelines)
			.values({
				name,
				description: (metadata.description as string) || '',
				tools: (metadata.tools as string[]) || [],
				inputFormats: (metadata.input_formats as string[]) || [],
				outputFormats: (metadata.output_formats as string[]) || [],
				tags: (metadata.tags as string[]) || [],
				githubUrl: github_url,
				metadataJson: metadata,
				author,
				version,
				verified: false,
				forkedFrom: resolvedForkedFrom,
				basedOnUrl: (metadata.based_on_url as string) || null,
				securityWarnings: approvedWarnings
			})
			.returning({ pipelineId: userPipelines.pipelineId });

		const pipelineId = row.pipelineId;

		// Self-reference guard: should never happen, but ensure forked_from != self
		if (resolvedForkedFrom === pipelineId) {
			await db
				.update(userPipelines)
				.set({ forkedFrom: null })
				.where(eq(userPipelines.pipelineId, pipelineId));
		}

		const response: Record<string, unknown> = {
			pipeline_id: pipelineId,
			name,
			author,
			security_warnings: approvedWarnings
		};

		return json(response, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};
