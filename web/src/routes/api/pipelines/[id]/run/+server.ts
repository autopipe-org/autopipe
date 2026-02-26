import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { db, schema } from '$lib/server/db.js';
import { eq, sql } from 'drizzle-orm';

const { userPipelines } = schema;

export const POST: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) return json({ error: 'Invalid ID' }, { status: 400 });

	await db
		.update(userPipelines)
		.set({ runCount: sql`${userPipelines.runCount} + 1` })
		.where(eq(userPipelines.pipelineId, id));

	return json({ success: true });
};
