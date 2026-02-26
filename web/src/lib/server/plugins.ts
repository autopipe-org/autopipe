import { db, schema } from './db.js';
import { eq, ilike, or, sql } from 'drizzle-orm';

const { userPlugins } = schema;

export interface PluginSummary {
	plugin_id: number;
	name: string;
	description: string;
	category: string;
	tags: string[];
	github_url: string;
	author: string;
	version: string;
	verified: boolean;
	forked_from: number | null;
	run_count: number;
	created_at: string | null;
}

export interface Plugin {
	plugin_id?: number;
	name: string;
	description: string;
	category: string;
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

function rowToSummary(r: typeof userPlugins.$inferSelect): PluginSummary {
	return {
		plugin_id: r.pluginId,
		name: r.name,
		description: r.description ?? '',
		category: r.category ?? '',
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

function rowToPlugin(r: typeof userPlugins.$inferSelect): Plugin {
	return {
		plugin_id: r.pluginId,
		name: r.name,
		description: r.description ?? '',
		category: r.category ?? '',
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

/** List plugins — only the latest version per name */
export async function listPlugins(): Promise<PluginSummary[]> {
	const rows = await db.execute(sql`
		SELECT DISTINCT ON (name) *
		FROM user_plugins
		ORDER BY name, created_at DESC
	`);
	return (rows.rows as (typeof userPlugins.$inferSelect)[]).map(rowToSummary);
}

/** Search plugins — only the latest version per name */
export async function searchPlugins(query: string): Promise<PluginSummary[]> {
	const pattern = `%${query}%`;
	const allRows = await db
		.select()
		.from(userPlugins)
		.where(
			or(
				ilike(userPlugins.name, pattern),
				ilike(userPlugins.description, pattern),
				ilike(userPlugins.category, pattern),
				sql`${query} = ANY(${userPlugins.tags})`
			)
		)
		.orderBy(sql`${userPlugins.createdAt} DESC`);

	// Deduplicate: keep only the latest per name
	const seen = new Set<string>();
	const deduped = allRows.filter((r) => {
		if (seen.has(r.name)) return false;
		seen.add(r.name);
		return true;
	});
	return deduped.map(rowToSummary);
}

export async function getPlugin(id: number): Promise<Plugin | null> {
	const rows = await db
		.select()
		.from(userPlugins)
		.where(eq(userPlugins.pluginId, id))
		.limit(1);
	if (rows.length === 0) return null;
	return rowToPlugin(rows[0]);
}

export async function insertPlugin(p: Plugin): Promise<number> {
	const [row] = await db
		.insert(userPlugins)
		.values({
			name: p.name,
			description: p.description,
			category: p.category,
			tags: p.tags,
			githubUrl: p.github_url,
			metadataJson: p.metadata_json,
			author: p.author,
			version: p.version,
			verified: p.verified
		})
		.returning({ pluginId: userPlugins.pluginId });
	return row.pluginId;
}

export async function updatePlugin(id: number, p: Plugin): Promise<boolean> {
	const result = await db
		.update(userPlugins)
		.set({
			description: p.description,
			category: p.category,
			tags: p.tags,
			githubUrl: p.github_url,
			metadataJson: p.metadata_json,
			author: p.author,
			version: p.version,
			verified: p.verified,
			updatedAt: new Date()
		})
		.where(eq(userPlugins.pluginId, id))
		.returning({ pluginId: userPlugins.pluginId });
	return result.length > 0;
}

export async function deletePlugin(id: number): Promise<boolean> {
	const result = await db
		.delete(userPlugins)
		.where(eq(userPlugins.pluginId, id))
		.returning({ pluginId: userPlugins.pluginId });
	return result.length > 0;
}

export async function getPluginByName(name: string): Promise<Plugin | null> {
	const rows = await db
		.select()
		.from(userPlugins)
		.where(eq(userPlugins.name, name))
		.orderBy(sql`${userPlugins.createdAt} DESC`)
		.limit(1);
	if (rows.length === 0) return null;
	return rowToPlugin(rows[0]);
}

export async function incrementRunCount(pluginId: number): Promise<void> {
	await db
		.update(userPlugins)
		.set({ runCount: sql`${userPlugins.runCount} + 1` })
		.where(eq(userPlugins.pluginId, pluginId));
}

/** Get all versions related to this plugin: same name + forked_from chain */
export async function getVersionChain(pluginId: number): Promise<PluginSummary[]> {
	const allRows = await db.select().from(userPlugins);
	const byId = new Map(allRows.map((r) => [r.pluginId, r]));

	const current = byId.get(pluginId);
	if (!current) return [];

	const chainIds = new Set<number>();

	// 1. All records with the same name
	for (const row of allRows) {
		if (row.name === current.name) {
			chainIds.add(row.pluginId);
		}
	}

	// 2. Walk up forked_from chain (covers cross-name forks)
	let walkId: number | null = pluginId;
	while (walkId !== null) {
		if (chainIds.has(walkId)) {
			const row = byId.get(walkId);
			if (!row || row.forkedFrom === null) break;
			walkId = row.forkedFrom;
			chainIds.add(walkId);
		} else {
			chainIds.add(walkId);
			const row = byId.get(walkId);
			if (!row) break;
			walkId = row.forkedFrom;
		}
	}

	// 3. Walk down: find children of any chain member
	let changed = true;
	while (changed) {
		changed = false;
		for (const row of allRows) {
			if (row.forkedFrom !== null && chainIds.has(row.forkedFrom) && !chainIds.has(row.pluginId)) {
				chainIds.add(row.pluginId);
				changed = true;
			}
		}
	}

	// Return sorted by created_at desc (newest first)
	const chainRows = allRows
		.filter((r) => chainIds.has(r.pluginId))
		.sort((a, b) => {
			const ta = a.createdAt?.getTime() ?? 0;
			const tb = b.createdAt?.getTime() ?? 0;
			return tb - ta;
		});
	return chainRows.map(rowToSummary);
}

export async function getDerivedPlugins(pluginId: number): Promise<PluginSummary[]> {
	const rows = await db
		.select()
		.from(userPlugins)
		.where(eq(userPlugins.forkedFrom, pluginId))
		.orderBy(sql`${userPlugins.createdAt} DESC`);
	return rows.map(rowToSummary);
}
