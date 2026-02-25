import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { env } from '$env/dynamic/private';

// POST /api/auth/device/poll — Poll for GitHub access token
export const POST: RequestHandler = async ({ request }) => {
	const clientId = env.GITHUB_CLIENT_ID;
	if (!clientId) {
		return json({ error: 'GitHub OAuth not configured' }, { status: 500 });
	}

	const body = await request.json();
	const deviceCode = body.device_code;
	if (!deviceCode) {
		return json({ error: 'device_code is required' }, { status: 400 });
	}

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
	} catch (e: unknown) {
		const message = e instanceof Error ? e.message : String(e);
		return json({ error: message }, { status: 500 });
	}
};
