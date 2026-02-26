import { db, schema } from './db.js';
import { eq, ilike, or, sql } from 'drizzle-orm';

const { userPipelines } = schema;

export interface PipelineSummary {
	pipeline_id: number;
	name: string;
	description: string;
	tools: string[];
	input_formats: string[];
	output_formats: string[];
	tags: string[];
	github_url: string;
	author: string;
	version: string;
	verified: boolean;
	forked_from: number | null;
	run_count: number;
	created_at: string | null;
}

export interface Pipeline {
	pipeline_id?: number;
	name: string;
	description: string;
	tools: string[];
	input_formats: string[];
	output_formats: string[];
	tags: string[];
	github_url: string;
	metadata_json: unknown;
	author: string;
	version: string;
	verified: boolean;
	forked_from?: number | null;
	run_count?: number;
	created_at?: string | null;
	updated_at?: string | null;
}

function rowToSummary(r: typeof userPipelines.$inferSelect): PipelineSummary {
	return {
		pipeline_id: r.pipelineId,
		name: r.name,
		description: r.description ?? '',
		tools: r.tools ?? [],
		input_formats: r.inputFormats ?? [],
		output_formats: r.outputFormats ?? [],
		tags: r.tags ?? [],
		github_url: r.githubUrl,
		author: r.author ?? '',
		version: r.version ?? '1.0.0',
		verified: r.verified ?? false,
		forked_from: r.forkedFrom ?? null,
		run_count: r.runCount ?? 0,
		created_at: r.createdAt?.toISOString() ?? null
	};
}

function rowToPipeline(r: typeof userPipelines.$inferSelect): Pipeline {
	return {
		pipeline_id: r.pipelineId,
		name: r.name,
		description: r.description ?? '',
		tools: r.tools ?? [],
		input_formats: r.inputFormats ?? [],
		output_formats: r.outputFormats ?? [],
		tags: r.tags ?? [],
		github_url: r.githubUrl,
		metadata_json: r.metadataJson,
		author: r.author ?? '',
		version: r.version ?? '1.0.0',
		verified: r.verified ?? false,
		forked_from: r.forkedFrom ?? null,
		run_count: r.runCount ?? 0,
		created_at: r.createdAt?.toISOString() ?? null,
		updated_at: r.updatedAt?.toISOString() ?? null
	};
}

export async function listPipelines(): Promise<PipelineSummary[]> {
	const rows = await db
		.select()
		.from(userPipelines)
		.orderBy(sql`${userPipelines.createdAt} DESC`);
	return rows.map(rowToSummary);
}

export async function searchPipelines(query: string): Promise<PipelineSummary[]> {
	const pattern = `%${query}%`;
	const rows = await db
		.select()
		.from(userPipelines)
		.where(
			or(
				ilike(userPipelines.name, pattern),
				ilike(userPipelines.description, pattern),
				sql`${query} = ANY(${userPipelines.tools})`,
				sql`${query} = ANY(${userPipelines.tags})`
			)
		)
		.orderBy(sql`${userPipelines.createdAt} DESC`);
	return rows.map(rowToSummary);
}

export async function getPipeline(id: number): Promise<Pipeline | null> {
	const rows = await db
		.select()
		.from(userPipelines)
		.where(eq(userPipelines.pipelineId, id))
		.limit(1);
	if (rows.length === 0) return null;
	return rowToPipeline(rows[0]);
}

export async function insertPipeline(p: Pipeline): Promise<number> {
	const [row] = await db
		.insert(userPipelines)
		.values({
			name: p.name,
			description: p.description,
			tools: p.tools,
			inputFormats: p.input_formats,
			outputFormats: p.output_formats,
			tags: p.tags,
			githubUrl: p.github_url,
			metadataJson: p.metadata_json,
			author: p.author,
			version: p.version,
			verified: p.verified
		})
		.returning({ pipelineId: userPipelines.pipelineId });
	return row.pipelineId;
}

export async function updatePipeline(id: number, p: Pipeline): Promise<boolean> {
	const result = await db
		.update(userPipelines)
		.set({
			description: p.description,
			tools: p.tools,
			inputFormats: p.input_formats,
			outputFormats: p.output_formats,
			tags: p.tags,
			githubUrl: p.github_url,
			metadataJson: p.metadata_json,
			author: p.author,
			version: p.version,
			verified: p.verified,
			updatedAt: new Date()
		})
		.where(eq(userPipelines.pipelineId, id))
		.returning({ pipelineId: userPipelines.pipelineId });
	return result.length > 0;
}

export async function deletePipeline(id: number): Promise<boolean> {
	const result = await db
		.delete(userPipelines)
		.where(eq(userPipelines.pipelineId, id))
		.returning({ pipelineId: userPipelines.pipelineId });
	return result.length > 0;
}

export async function incrementRunCount(pipelineId: number): Promise<void> {
	await db
		.update(userPipelines)
		.set({ runCount: sql`${userPipelines.runCount} + 1` })
		.where(eq(userPipelines.pipelineId, pipelineId));
}

export async function getVersionChain(pipelineId: number): Promise<PipelineSummary[]> {
	// 1. Follow forked_from upward to find the root
	const allRows = await db.select().from(userPipelines);
	const byId = new Map(allRows.map((r) => [r.pipelineId, r]));

	// Walk up the fork chain to find the root
	let currentId: number | null = pipelineId;
	const chainIds = new Set<number>();
	while (currentId !== null) {
		if (chainIds.has(currentId)) break; // prevent cycles
		chainIds.add(currentId);
		const row = byId.get(currentId);
		if (!row) break;
		currentId = row.forkedFrom ?? null;
	}

	// 2. Walk down: find all pipelines whose forked_from is in the chain
	let changed = true;
	while (changed) {
		changed = false;
		for (const row of allRows) {
			if (row.forkedFrom !== null && chainIds.has(row.forkedFrom) && !chainIds.has(row.pipelineId)) {
				chainIds.add(row.pipelineId);
				changed = true;
			}
		}
	}

	// 3. Return sorted by created_at desc
	const chainRows = allRows
		.filter((r) => chainIds.has(r.pipelineId))
		.sort((a, b) => {
			const ta = a.createdAt?.getTime() ?? 0;
			const tb = b.createdAt?.getTime() ?? 0;
			return tb - ta;
		});
	return chainRows.map(rowToSummary);
}

export async function getDerivedPipelines(pipelineId: number): Promise<PipelineSummary[]> {
	const rows = await db
		.select()
		.from(userPipelines)
		.where(eq(userPipelines.forkedFrom, pipelineId))
		.orderBy(sql`${userPipelines.createdAt} DESC`);
	return rows.map(rowToSummary);
}
