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
	author: string;
	version: string;
	verified: boolean;
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
	snakefile: string;
	dockerfile: string;
	config_yaml: string;
	metadata_json: unknown;
	readme: string;
	author: string;
	version: string;
	verified: boolean;
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
		author: r.author ?? '',
		version: r.version ?? '1.0.0',
		verified: r.verified ?? false,
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
		snakefile: r.snakefile,
		dockerfile: r.dockerfile,
		config_yaml: r.configYaml ?? '',
		metadata_json: r.metadataJson,
		readme: r.readme ?? '',
		author: r.author ?? '',
		version: r.version ?? '1.0.0',
		verified: r.verified ?? false,
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
			snakefile: p.snakefile,
			dockerfile: p.dockerfile,
			configYaml: p.config_yaml,
			metadataJson: p.metadata_json,
			readme: p.readme,
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
			snakefile: p.snakefile,
			dockerfile: p.dockerfile,
			configYaml: p.config_yaml,
			metadataJson: p.metadata_json,
			readme: p.readme,
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
