export interface FileTreeEntry {
	path: string;
	type: 'blob' | 'tree';
}

export interface GithubTree {
	files: FileTreeEntry[];
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

// Cache: key -> { data, timestamp }
const treeCache = new Map<string, { tree: GithubTree; ts: number }>();
const fileCache = new Map<string, { content: string; ts: number }>();
const CACHE_TTL = 5 * 60 * 1000; // 5 minutes

const GITHUB_TOKEN = process.env.GITHUB_TOKEN || '';

function githubHeaders(): Record<string, string> {
	const h: Record<string, string> = {
		'User-Agent': 'autopipe-registry'
	};
	if (GITHUB_TOKEN) {
		h['Authorization'] = `Bearer ${GITHUB_TOKEN}`;
	}
	return h;
}

export function parseGithubUrl(url: string): ParsedUrl {
	const match = url.match(/github\.com\/([^/]+)\/([^/]+)\/tree\/[^/]+\/(.+)/);
	if (match) {
		return { owner: match[1], repo: match[2], path: match[3] };
	}
	const repoMatch = url.match(/github\.com\/([^/]+)\/([^/]+)\/?$/);
	if (repoMatch) {
		return { owner: repoMatch[1], repo: repoMatch[2], path: '' };
	}
	throw new Error(`Invalid GitHub URL format: ${url}`);
}

/** Fetch the file tree for a pipeline directory using GitHub Trees API. */
export async function fetchGithubTree(githubUrl: string, ref = 'main'): Promise<GithubTree> {
	const cacheKey = `${githubUrl}@${ref}`;
	const cached = treeCache.get(cacheKey);
	if (cached && Date.now() - cached.ts < CACHE_TTL) {
		return cached.tree;
	}

	const { owner, repo, path } = parseGithubUrl(githubUrl);

	const treeUrl = `https://api.github.com/repos/${owner}/${repo}/git/trees/${ref}?recursive=1`;
	const resp = await fetch(treeUrl, { headers: githubHeaders() });

	if (resp.status === 404) {
		throw new GithubNotFoundError(githubUrl);
	}
	if (!resp.ok) {
		throw new Error(`GitHub API error ${resp.status}: ${await resp.text()}`);
	}

	const body = await resp.json();
	const prefix = path ? `${path}/` : '';

	const files: FileTreeEntry[] = (body.tree || [])
		.filter((entry: any) => {
			const p = entry.path as string;
			if (!prefix) return true;
			return p.startsWith(prefix);
		})
		.map((entry: any) => ({
			path: prefix ? (entry.path as string).slice(prefix.length) : entry.path,
			type: entry.type as 'blob' | 'tree'
		}))
		.filter((entry: FileTreeEntry) => entry.path.length > 0);

	const tree: GithubTree = { files };
	treeCache.set(cacheKey, { tree, ts: Date.now() });
	return tree;
}

/** Fetch a single file's content from GitHub. */
export async function fetchGithubFile(
	githubUrl: string,
	filePath: string,
	ref?: string
): Promise<string> {
	const cacheKey = `${githubUrl}/${filePath}@${ref || 'latest'}`;
	const cached = fileCache.get(cacheKey);
	if (cached && Date.now() - cached.ts < CACHE_TTL) {
		return cached.content;
	}

	const { owner, repo, path } = parseGithubUrl(githubUrl);
	const fullPath = path ? `${path}/${filePath}` : filePath;
	let apiUrl = `https://api.github.com/repos/${owner}/${repo}/contents/${fullPath}`;
	if (ref) apiUrl += `?ref=${encodeURIComponent(ref)}`;

	const resp = await fetch(apiUrl, {
		headers: {
			...githubHeaders(),
			Accept: 'application/vnd.github.raw'
		}
	});

	if (resp.status === 404) return '';
	if (!resp.ok) {
		throw new Error(`GitHub API error ${resp.status}: ${await resp.text()}`);
	}

	const content = await resp.text();
	fileCache.set(cacheKey, { content, ts: Date.now() });
	return content;
}

/** Fetch all files for ZIP download. */
export async function fetchAllGithubFiles(
	githubUrl: string,
	ref = 'main'
): Promise<{ path: string; content: string }[]> {
	const tree = await fetchGithubTree(githubUrl, ref);
	const blobs = tree.files.filter((f) => f.type === 'blob');

	const results: { path: string; content: string }[] = [];
	// Fetch in batches of 10 to avoid rate limiting
	for (let i = 0; i < blobs.length; i += 10) {
		const batch = blobs.slice(i, i + 10);
		const contents = await Promise.all(
			batch.map((f) => fetchGithubFile(githubUrl, f.path, ref))
		);
		batch.forEach((f, idx) => {
			if (contents[idx]) {
				results.push({ path: f.path, content: contents[idx] });
			}
		});
	}

	return results;
}
