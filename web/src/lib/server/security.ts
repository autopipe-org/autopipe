// Hard-layer security validation — runs server-side as a deterministic gate.
// Only critical patterns that have NO legitimate use in a pipeline are listed
// here. Soft warnings (chmod 777, --network host, etc.) are intentionally
// removed — those are now surfaced by the MCP client's AI review (Claude or
// whatever LLM the user is using) which presents them with an explanation
// and asks for explicit approval before publish.
//
// Scope: Snakefile + Dockerfile only. This matches the existing behaviour
// and keeps the deterministic gate narrow to avoid false positives in
// scripts/*.py and similar files where the AI review provides better
// context-aware judgement.

export interface SecurityIssue {
	line: number;
	file: string;
	severity: 'error';
	pattern_id: string;
	message: string;
}

interface Pattern {
	id: string;
	regex: RegExp;
	message: string;
}

const ERROR_PATTERNS: Pattern[] = [
	// Destructive deletion
	{
		id: 'rm-rf-recursive',
		regex: /rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|(-[a-zA-Z]*f[a-zA-Z]*r))\b/,
		message: 'Destructive recursive force delete (rm -rf)'
	},
	// Disk formatting / raw writes
	{ id: 'mkfs', regex: /\bmkfs\b/, message: 'Disk formatting command (mkfs)' },
	{ id: 'dd-write', regex: /\bdd\s+if=/, message: 'Raw disk write (dd)' },
	{ id: 'dev-block-write', regex: />\s*\/dev\/sd/, message: 'Writing to block device' },
	// Remote code execution (allow nextflow installer specifically)
	{
		id: 'curl-pipe-shell',
		regex: /curl\s(?!.*get\.nextflow\.io).*\|\s*(sh|bash|zsh)/,
		message: 'Remote code execution (curl | shell)'
	},
	{
		id: 'wget-pipe-shell',
		regex: /wget\s.*\|\s*(sh|bash|zsh)/,
		message: 'Remote code execution (wget | shell)'
	},
	// Privilege escalation / system control
	{ id: 'docker-privileged', regex: /--privileged/, message: 'Docker privileged mode' },
	{
		id: 'system-shutdown',
		regex: /\b(shutdown|reboot|halt|poweroff)\b/,
		message: 'System shutdown/reboot command'
	},
	{ id: 'kill-init', regex: /\bkill\s+-9\s+1\b/, message: 'Killing init process' },
	{
		id: 'fork-bomb',
		regex: /:\(\)\s*\{\s*:\|:&\s*\};\s*:/,
		message: 'Fork bomb detected'
	}
];

function extractShellBlocks(snakefile: string): { line: number; content: string }[] {
	const blocks: { line: number; content: string }[] = [];
	const lines = snakefile.split('\n');
	let inShell = false;
	let shellContent = '';
	let shellStart = 0;
	let indent = 0;

	for (let i = 0; i < lines.length; i++) {
		const line = lines[i];
		const trimmed = line.trimStart();

		if (trimmed.startsWith('shell:')) {
			inShell = true;
			shellStart = i + 1;
			indent = line.length - trimmed.length;
			const inline = trimmed.slice(6).trim();
			if (inline) {
				shellContent = inline;
			}
			continue;
		}

		if (inShell) {
			const currentIndent = line.length - line.trimStart().length;
			if (trimmed === '' || currentIndent > indent) {
				shellContent += '\n' + line;
			} else {
				blocks.push({ line: shellStart, content: shellContent });
				shellContent = '';
				inShell = false;
			}
		}
	}

	if (inShell && shellContent) {
		blocks.push({ line: shellStart, content: shellContent });
	}

	return blocks;
}

function extractRunCommands(dockerfile: string): { line: number; content: string }[] {
	const commands: { line: number; content: string }[] = [];
	const lines = dockerfile.split('\n');

	for (let i = 0; i < lines.length; i++) {
		const trimmed = lines[i].trimStart();
		if (/^RUN\s/i.test(trimmed)) {
			let cmd = trimmed.slice(4).trim();
			// Handle multi-line RUN with backslash
			let j = i;
			while (cmd.endsWith('\\') && j + 1 < lines.length) {
				j++;
				cmd = cmd.slice(0, -1) + ' ' + lines[j].trim();
			}
			commands.push({ line: i + 1, content: cmd });
		}
	}

	return commands;
}

export function validateSecurity(snakefile: string, dockerfile: string): SecurityIssue[] {
	const issues: SecurityIssue[] = [];

	// Snakefile shell: blocks
	for (const block of extractShellBlocks(snakefile)) {
		for (const pattern of ERROR_PATTERNS) {
			if (pattern.regex.test(block.content)) {
				issues.push({
					line: block.line,
					file: 'Snakefile',
					severity: 'error',
					pattern_id: pattern.id,
					message: pattern.message
				});
			}
		}
	}

	// Snakefile lines outside shell: blocks that invoke os.system / subprocess.*
	const snakeLines = snakefile.split('\n');
	for (let i = 0; i < snakeLines.length; i++) {
		const line = snakeLines[i].trimStart();
		if (line.startsWith('shell:') || line.startsWith('#')) continue;
		if (/\bos\.system\(|subprocess\.(call|run|Popen)\(/.test(line)) {
			for (const pattern of ERROR_PATTERNS) {
				if (pattern.regex.test(line)) {
					issues.push({
						line: i + 1,
						file: 'Snakefile',
						severity: 'error',
						pattern_id: pattern.id,
						message: pattern.message
					});
				}
			}
		}
	}

	// Dockerfile RUN commands.
	// Note: the same pattern set runs here. Remote-code-execution patterns
	// (curl|wget piped to a shell) are kept as 'error' rather than being
	// downgraded — the previous warning-level downgrade went away with the
	// soft warning patterns. The legitimate Nextflow installer is still
	// allowed via the negative lookahead in `curl-pipe-shell`.
	for (const cmd of extractRunCommands(dockerfile)) {
		for (const pattern of ERROR_PATTERNS) {
			if (pattern.regex.test(cmd.content)) {
				issues.push({
					line: cmd.line,
					file: 'Dockerfile',
					severity: 'error',
					pattern_id: pattern.id,
					message: pattern.message
				});
			}
		}
	}

	return issues;
}

export function hasErrors(issues: SecurityIssue[]): boolean {
	return issues.length > 0;
}

/**
 * Validate a GitHub token by fetching the authenticated user.
 * Returns the GitHub username on success, or null on failure.
 */
export async function validateGithubToken(token: string): Promise<string | null> {
	if (!token) return null;
	try {
		const resp = await fetch('https://api.github.com/user', {
			headers: {
				Authorization: `Bearer ${token}`,
				'User-Agent': 'autopipe-registry'
			}
		});
		if (!resp.ok) return null;
		const user = await resp.json();
		return (user.login as string) || null;
	} catch {
		return null;
	}
}

/**
 * Extract Bearer token from Authorization header.
 */
export function extractBearerToken(request: Request): string | null {
	const auth = request.headers.get('Authorization');
	if (!auth) return null;
	const match = auth.match(/^Bearer\s+(.+)$/i);
	return match ? match[1] : null;
}

/**
 * Sanitize an error message to prevent leaking internal details.
 */
export function sanitizeErrorMessage(message: string): string {
	if (
		message.includes('relation') ||
		message.includes('syntax error') ||
		message.includes('ECONNREFUSED') ||
		message.includes('password authentication')
	) {
		return 'Internal server error';
	}
	return message;
}

/**
 * Shape of an AI-detected suspicious pattern stored alongside a published
 * pipeline. Each entry comes from the MCP client's code review and has been
 * explicitly approved by the publisher; the same list is shown to anyone
 * downloading the pipeline so they can decide whether to accept the same
 * risks.
 */
export interface AiSecurityWarning {
	file: string;
	line: number;
	code_snippet: string;
	concern: string;
	category?: string;
}

/** Lightweight runtime validation — accept anything shaped like the type. */
export function normalizeAiWarnings(input: unknown): AiSecurityWarning[] {
	if (!Array.isArray(input)) return [];
	const out: AiSecurityWarning[] = [];
	for (const raw of input) {
		if (!raw || typeof raw !== 'object') continue;
		const r = raw as Record<string, unknown>;
		const file = typeof r.file === 'string' ? r.file : '';
		const line = typeof r.line === 'number' ? r.line : 0;
		const code_snippet = typeof r.code_snippet === 'string' ? r.code_snippet : '';
		const concern = typeof r.concern === 'string' ? r.concern : '';
		if (!file || !code_snippet || !concern) continue;
		const category = typeof r.category === 'string' ? r.category : undefined;
		out.push({ file, line, code_snippet, concern, category });
	}
	return out;
}
