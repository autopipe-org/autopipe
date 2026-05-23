import {
	pgTable,
	serial,
	varchar,
	char,
	text,
	integer,
	boolean,
	timestamp,
	jsonb,
	index
} from 'drizzle-orm/pg-core';

export const userPipelines = pgTable(
	'user_pipelines',
	{
		pipelineId: serial('pipeline_id').primaryKey(),
		name: varchar('name', { length: 255 }).notNull(),
		description: text('description'),
		tools: text('tools').array(),
		inputFormats: text('input_formats').array(),
		outputFormats: text('output_formats').array(),
		tags: text('tags').array(),
		githubUrl: varchar('github_url', { length: 500 }).notNull(),
		metadataJson: jsonb('metadata_json').notNull(),
		author: varchar('author', { length: 255 }),
		version: varchar('version', { length: 50 }).default('1.0.0'),
		verified: boolean('verified').default(false),
		forkedFrom: integer('forked_from'),
		basedOnUrl: varchar('based_on_url', { length: 500 }),
		// AI-detected suspicious patterns the publisher explicitly approved.
		// Each entry: { file, line, code_snippet, concern, category? }.
		// Shown to downloaders so they can decide whether to accept the same
		// risks. Empty array means no warnings were surfaced at publish.
		securityWarnings: jsonb('security_warnings').default([]),
		// Git tag and commit SHA captured at publish time. NULL on legacy
		// rows; download / Hub rendering falls back to the default branch
		// when these are NULL.
		gitTag: varchar('git_tag', { length: 255 }),
		commitSha: char('commit_sha', { length: 40 }),
		createdAt: timestamp('created_at').defaultNow(),
		updatedAt: timestamp('updated_at').defaultNow()
	},
	(table) => [
		index('idx_user_pipelines_name').on(table.name),
		index('idx_user_pipelines_git_tag').on(table.gitTag)
	]
);

export const userPlugins = pgTable(
	'user_plugins',
	{
		pluginId: serial('plugin_id').primaryKey(),
		name: varchar('name', { length: 255 }).notNull(),
		description: text('description'),
		category: varchar('category', { length: 100 }),
		extensions: text('extensions').array().default([]),
		tags: text('tags').array(),
		githubUrl: varchar('github_url', { length: 500 }).notNull(),
		metadataJson: jsonb('metadata_json').notNull(),
		readme: text('readme'),
		author: varchar('author', { length: 255 }),
		version: varchar('version', { length: 50 }).default('1.0.0'),
		verified: boolean('verified').default(false),
		forkedFrom: integer('forked_from'),
		versionHistory: jsonb('version_history').default([]),
		createdAt: timestamp('created_at').defaultNow(),
		updatedAt: timestamp('updated_at').defaultNow()
	},
	(table) => [
		index('idx_user_plugins_name').on(table.name),
	]
);
