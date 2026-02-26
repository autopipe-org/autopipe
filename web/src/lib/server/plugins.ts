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

export async function listPlugins(): Promise<PluginSummary[]> {
	const rows = await db
		.select()
		.from(userPlugins)
		.orderBy(sql`${userPlugins.createdAt} DESC`);
	return rows.map(rowToSummary);
}

export async function searchPlugins(query: string): Promise<PluginSummary[]> {
	const pattern = `%${query}%`;
	const rows = await db
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
	return rows.map(rowToSummary);
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

export async function getVersionChain(pluginId: number): Promise<PluginSummary[]> {
	// 1. Follow forked_from upward to find the root
	const allRows = await db.select().from(userPlugins);
	const byId = new Map(allRows.map((r) => [r.pluginId, r]));

	// Walk up the fork chain to find the root
	let currentId: number | null = pluginId;
	const chainIds = new Set<number>();
	while (currentId !== null) {
		if (chainIds.has(currentId)) break; // prevent cycles
		chainIds.add(currentId);
		const row = byId.get(currentId);
		if (!row) break;
		currentId = row.forkedFrom ?? null;
	}

	// 2. Walk down: find all plugins whose forked_from is in the chain
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

	// 3. Return sorted by created_at desc
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
