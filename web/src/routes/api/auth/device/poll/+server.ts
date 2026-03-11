import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { env } from '$env/dynamic/private';

// Simple per-device_code rate limiter: min 4s between polls (GitHub recommends 5s)
const lastPoll = new Map<string, number>();
const POLL_INTERVAL_MS = 4000;
const MAX_RATE_LIMIT_ENTRIES = 10000;

// Cleanup stale entries every 5 minutes
setInterval(() => {
	const cutoff = Date.now() - 10 * 60 * 1000;
	for (const [key, ts] of lastPoll) {
		if (ts < cutoff) lastPoll.delete(key);
	}
}, 5 * 60 * 1000);

// POST /api/auth/device/poll — Poll for GitHub access token
export const POST: RequestHandler = async ({ request }) => {
	const clientId = env.GITHUB_CLIENT_ID;
	if (!clientId) {
		return json({ error: 'GitHub OAuth not configured' }, { status: 500 });
	}

	let body: Record<string, unknown>;
	try {
		body = await request.json();
	} catch {
		return json({ error: 'Invalid request body' }, { status: 400 });
	}
	const deviceCode = body.device_code;
	if (!deviceCode || typeof deviceCode !== 'string') {
		return json({ error: 'device_code is required' }, { status: 400 });
	}

	// Rate limit: reject if polling too fast for this device_code
	const now = Date.now();
	const last = lastPoll.get(deviceCode);
	if (last && now - last < POLL_INTERVAL_MS) {
		return json({ error: 'slow_down', message: 'Polling too fast. Wait at least 5 seconds.' }, { status: 429 });
	}
	// Evict oldest entry if map exceeds size limit
	if (lastPoll.size >= MAX_RATE_LIMIT_ENTRIES) {
		const oldest = [...lastPoll.entries()].reduce((a, b) => (a[1] < b[1] ? a : b));
		lastPoll.delete(oldest[0]);
	}
	lastPoll.set(deviceCode, now);

	try {
		const resp = await fetch('https://github.com/login/oauth/access_token', {
			method: 'POST',
			headers: {
				Accept: 'application/json',
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({
				client_id: clientId,
				device_code: deviceCode,
				grant_type: 'urn:ietf:params:oauth:grant-type:device_code'
			})
		});

		const data = await resp.json();

		if (data.access_token) {
			return json({ access_token: data.access_token });
		}

		// Still pending or error
		return json({ error: data.error || 'unknown_error' });
	} catch {
		return json({ error: 'Failed to poll GitHub device flow' }, { status: 500 });
	}
};
