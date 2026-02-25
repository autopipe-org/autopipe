import type { RequestHandler } from './$types';
import { error } from '@sveltejs/kit';
import { getPipeline } from '$lib/server/pipelines.js';
import JSZip from 'jszip';

export const GET: RequestHandler = async ({ params }) => {
	const id = parseInt(params.id);
	if (isNaN(id)) throw error(400, 'Invalid pipeline ID');

	const pipeline = await getPipeline(id);
	if (!pipeline) throw error(404, 'Pipeline not found');

	const zip = new JSZip();
	const folder = zip.folder(pipeline.name)!;

	const files: [string, string][] = [
		['Snakefile', pipeline.snakefile],
		['Dockerfile', pipeline.dockerfile],
		['config.yaml', pipeline.config_yaml],
		[
			'metadata.json',
			typeof pipeline.metadata_json === 'string'
				? pipeline.metadata_json
				: JSON.stringify(pipeline.metadata_json, null, 2)
		],
		['README.md', pipeline.readme]
	];

	for (const [name, content] of files) {
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
