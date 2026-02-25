import {
	pgTable,
	serial,
	varchar,
	text,
	boolean,
	timestamp,
	jsonb,
	index
} from 'drizzle-orm/pg-core';

export const userPipelines = pgTable(
	'user_pipelines',
	{
		pipelineId: serial('pipeline_id').primaryKey(),
		name: varchar('name', { length: 255 }).notNull().unique(),
		description: text('description'),
		tools: text('tools').array(),
		inputFormats: text('input_formats').array(),
		outputFormats: text('output_formats').array(),
		tags: text('tags').array(),
		snakefile: text('snakefile').notNull(),
		dockerfile: text('dockerfile').notNull(),
		configYaml: text('config_yaml'),
		metadataJson: jsonb('metadata_json').notNull(),
		readme: text('readme'),
		author: varchar('author', { length: 255 }),
		version: varchar('version', { length: 50 }).default('1.0.0'),
		verified: boolean('verified').default(false),
		createdAt: timestamp('created_at').defaultNow(),
		updatedAt: timestamp('updated_at').defaultNow()
	},
	(table) => [
		index('idx_user_pipelines_name').on(table.name),
	]
);
