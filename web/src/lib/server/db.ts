import { drizzle } from 'drizzle-orm/node-postgres';
import pg from 'pg';
import * as schema from './schema.js';

const pool = new pg.Pool({
	host: process.env.POSTGRES_HOST || 'localhost',
	port: parseInt(process.env.POSTGRES_PORT || '5433'),
	database: process.env.POSTGRES_DB || 'autopipe_wf',
	user: process.env.POSTGRES_USER || 'autopipe',
	password: process.env.POSTGRES_PASSWORD || 'autopipe123'
});

export const db = drizzle(pool, { schema });
export { schema };
