import { json } from '@sveltejs/kit';
import type { RequestHandler } from './$types';
import { env } from '$env/dynamic/private';

// POST /api/auth/device — Start GitHub device flow
export const POST: RequestHandler = async () => {
	const clientId = env.GITHUB_CLIENT_ID;
	if (!clientId) {
		return json({ error: 'GitHub OAuth not configured' }, { status: 500 });
	}

	try {
		const resp = await fetch('https://github.com/login/device/code', {
			method: 'POST',
			headers: {
				Accept: 'application/json',
				'Content-Type': 'application/json'
			},
			body: JSON.stringify({
				client_id: clientId,
				scope: 'repo'
			})
		});

		const data = await resp.json();
		if (data.error) {
			return json({ error: data.error_description || data.error }, { status: 400 });
		}

		return json({
			device_code: data.device_code,
			user_code: data.user_code,
			verification_uri: data.verification_uri,
			interval: data.interval || 5
		});
	} catch {
		return json({ error: 'Failed to initiate GitHub device flow' }, { status: 500 });
	}
};
