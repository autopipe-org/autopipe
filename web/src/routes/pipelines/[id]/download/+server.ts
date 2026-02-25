import type { RequestHandler } from './$types';
import { error } from '@sveltejs/kit';
import { getPipeline, deletePipeline } from '$lib/server/pipelines.js';
import { fetchGithubFiles, GithubNotFoundError } from '$lib/server/github.js';
import JSZip from 'jszip';

export const GET: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	// Fetch files from GitHub
	let files;
	try {
		files = await fetchGithubFiles(pipeline.github_url);
	} catch (e) {
		if (e instanceof GithubNotFoundError) {
			await deletePipeline(id);
			throw error(404, 'Pipeline no longer exists on GitHub and has been removed');
		}
		throw error(502, `Failed to fetch from GitHub: ${e instanceof Error ? e.message : String(e)}`);
	}

	const zip = new JSZip();
	const folder = zip.folder(pipeline.name)!;

	const entries: [string, string][] = [
		['Snakefile', files.snakefile],
		['Dockerfile', files.dockerfile],
		['config.yaml', files.config_yaml],
		['metadata.json', files.metadata_json],
		['README.md', files.readme]
	];

	for (const [name, content] of entries) {
		if (content) {
			folder.file(name, content);
		}
	}

	const buf = await zip.generateAsync({ type: 'arraybuffer', compression: 'DEFLATE' });

	return new Response(buf, {
		headers: {
			'Content-Type': 'application/zip',
			'Content-Disposition': `attachment; filename="${pipeline.name}.zip"`
		}
	});
};
