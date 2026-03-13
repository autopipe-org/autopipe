import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { validateSecurity, hasErrors, sanitizeErrorMessage } from '$lib/server/security.js';
import { fetchGithubFile } from '$lib/server/github.js';
import { db, schema } from '$lib/server/db.js';
import { eq, sql } from 'drizzle-orm';

const { userPipelines } = schema;

// POST /api/publish — Fetch code from GitHub, validate security, store URL + metadata
export const POST: RequestHandler = async ({ request }) => {
	try {
		const body = await request.json();
		const { github_url, github_token, forked_from } = body;

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

		// 2. Fetch required files from GitHub for validation
		let snakefile: string, dockerfile: string, metadata_json: string;
		try {
			[snakefile, dockerfile, metadata_json] = await Promise.all([
				fetchGithubFile(github_url, 'Snakefile'),
				fetchGithubFile(github_url, 'Dockerfile'),
				fetchGithubFile(github_url, 'ro-crate-metadata.json')
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

		// 5. Security validation
		const issues = validateSecurity(snakefile, dockerfile);
		if (hasErrors(issues)) {
			return json({ error: 'Security validation failed', issues }, { status: 422 });
		}

		// 6. Always INSERT a new record (version tracking)
		let name = metadata.name as string;
		let version = (metadata.version as string) || '1.0.0';

		// forked_from: trust the value Claude sends (no auto-detection)
		const resolvedForkedFrom: number | null =
			typeof forked_from === 'number' ? forked_from : null;

		if (resolvedForkedFrom !== null) {
			// forked_from specified → check original author
			const [parent] = await db
				.select({ author: userPipelines.author, name: userPipelines.name })
				.from(userPipelines)
				.where(eq(userPipelines.pipelineId, resolvedForkedFrom))
				.limit(1);

			if (parent && parent.author === author) {
				// Same author → version upgrade: use the original pipeline's name
				name = parent.name;
			} else {
				// Different author → fork: independent version chain starting at 1.0.0
				version = '1.0.0';
			}
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
				basedOnUrl: (metadata.based_on_url as string) || null
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

		const response: Record<string, unknown> = { pipeline_id: pipelineId, name, author };
		if (issues.length > 0) {
			response.warnings = issues;
		}

		return json(response, { status: 200 });
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: sanitizeErrorMessage(message) }, { status: 500 });
	}
};
