import { drizzle } from 'drizzle-orm/node-postgres';
import pg from 'pg';
import * as schema from './schema.js';

if (!process.env.POSTGRES_PASSWORD) {
	throw new Error('POSTGRES_PASSWORD environment variable is required');
}

const pool = new pg.Pool({
	host: process.env.POSTGRES_HOST || 'localhost',
	port: parseInt(process.env.POSTGRES_PORT || '5433'),
	database: process.env.POSTGRES_DB || 'autopipe_wf',
	user: process.env.POSTGRES_USER || 'autopipe',
	password: process.env.POSTGRES_PASSWORD,
	ssl: process.env.POSTGRES_SSL === 'true' ? { rejectUnauthorized: false } : undefined
});

export const db = drizzle(pool, { schema });
export { schema };
