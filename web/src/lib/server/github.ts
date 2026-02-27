export interface GithubFiles {
	snakefile: string;
	dockerfile: string;
	config_yaml: string;
	metadata_json: string;
	readme: string;
}

export class GithubNotFoundError extends Error {
	constructor(url: string) {
		super(`GitHub repository or files not found: ${url}`);
		this.name = 'GithubNotFoundError';
	}
}

interface ParsedUrl {
	owner: string;
	repo: string;
	path: string;
}

// Cache: github_url -> { files, timestamp }
const cache = new Map<string, { files: GithubFiles; ts: number }>();
const CACHE_TTL = 5 * 60 * 1000; // 5 minutes

export function parseGithubUrl(url: string): ParsedUrl {
	// Formats:
	// https://github.com/{owner}/{repo}/tree/{branch}/{path}
	// https://github.com/{owner}/{repo}/tree/main/{path}
	const match = url.match(/github\.com\/([^/]+)\/([^/]+)\/tree\/[^/]+\/(.+)/);
	if (match) {
		return { owner: match[1], repo: match[2], path: match[3] };
	}

	// https://github.com/{owner}/{repo} (root)
	const repoMatch = url.match(/github\.com\/([^/]+)\/([^/]+)\/?$/);
	if (repoMatch) {
		return { owner: repoMatch[1], repo: repoMatch[2], path: '' };
	}

	throw new Error(`Invalid GitHub URL format: ${url}`);
}

async function fetchFile(
	owner: string,
	repo: string,
	path: string,
	filename: string,
	ref?: string
): Promise<string> {
	const filePath = path ? `${path}/${filename}` : filename;
	let apiUrl = `https://api.github.com/repos/${owner}/${repo}/contents/${filePath}`;
	if (ref) apiUrl += `?ref=${encodeURIComponent(ref)}`;

	const resp = await fetch(apiUrl, {
		headers: {
			Accept: 'application/vnd.github.raw',
			'User-Agent': 'autopipe-registry'
		}
	});

	if (resp.status === 404) {
		return '';
	}
	if (!resp.ok) {
		throw new Error(`GitHub API error ${resp.status}: ${await resp.text()}`);
	}

	return await resp.text();
}

export async function fetchGithubFiles(githubUrl: string): Promise<GithubFiles> {
	// Check cache
	const cached = cache.get(githubUrl);
	if (cached && Date.now() - cached.ts < CACHE_TTL) {
		return cached.files;
	}

	const { owner, repo, path } = parseGithubUrl(githubUrl);

	// Verify repo/path exists
	const checkPath = path
		? `https://api.github.com/repos/${owner}/${repo}/contents/${path}`
		: `https://api.github.com/repos/${owner}/${repo}`;
	const checkResp = await fetch(checkPath, {
		headers: { 'User-Agent': 'autopipe-registry' }
	});

	if (checkResp.status === 404) {
		throw new GithubNotFoundError(githubUrl);
	}

	const [snakefile, dockerfile, config_yaml, metadata_json, readme] = await Promise.all([
		fetchFile(owner, repo, path, 'Snakefile'),
		fetchFile(owner, repo, path, 'Dockerfile'),
		fetchFile(owner, repo, path, 'config.yaml'),
		fetchFile(owner, repo, path, 'metadata.json'),
		fetchFile(owner, repo, path, 'README.md')
	]);

	const files: GithubFiles = { snakefile, dockerfile, config_yaml, metadata_json, readme };

	// Update cache
	cache.set(githubUrl, { files, ts: Date.now() });

	return files;
}

/** Fetch pipeline files at a specific git ref (tag or commit). */
export async function fetchGithubFilesAtRef(
	githubUrl: string,
	ref: string
): Promise<GithubFiles> {
	const cacheKey = `${githubUrl}@${ref}`;
	const cached = cache.get(cacheKey);
	if (cached && Date.now() - cached.ts < CACHE_TTL) {
		return cached.files;
	}

	const { owner, repo, path } = parseGithubUrl(githubUrl);

	const [snakefile, dockerfile, config_yaml, metadata_json, readme] = await Promise.all([
		fetchFile(owner, repo, path, 'Snakefile', ref),
		fetchFile(owner, repo, path, 'Dockerfile', ref),
		fetchFile(owner, repo, path, 'config.yaml', ref),
		fetchFile(owner, repo, path, 'metadata.json', ref),
		fetchFile(owner, repo, path, 'README.md', ref)
	]);

	const files: GithubFiles = { snakefile, dockerfile, config_yaml, metadata_json, readme };
	cache.set(cacheKey, { files, ts: Date.now() });
	return files;
}
