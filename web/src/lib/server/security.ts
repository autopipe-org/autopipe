export interface SecurityIssue {
	line: number;
	file: string;
	severity: 'error' | 'warning';
	message: string;
}

interface Pattern {
	regex: RegExp;
	severity: 'error' | 'warning';
	message: string;
}

const SHELL_PATTERNS: Pattern[] = [
	// Destructive deletion
	{ regex: /rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|(-[a-zA-Z]*f[a-zA-Z]*r))\b/, severity: 'error', message: 'Destructive recursive force delete (rm -rf)' },
	{ regex: /rm\s+-[a-zA-Z]*f\b/, severity: 'warning', message: 'Force delete (rm -f) — verify this is safe' },
	// Disk formatting / raw writes
	{ regex: /\bmkfs\b/, severity: 'error', message: 'Disk formatting command (mkfs)' },
	{ regex: /\bdd\s+if=/, severity: 'error', message: 'Raw disk write (dd)' },
	{ regex: />\s*\/dev\/sd/, severity: 'error', message: 'Writing to block device' },
	// Remote code execution
	{ regex: /curl\s(?!.*get\.nextflow\.io).*\|\s*(sh|bash|zsh)/, severity: 'error', message: 'Remote code execution (curl | shell)' },
	{ regex: /wget\s.*\|\s*(sh|bash|zsh)/, severity: 'error', message: 'Remote code execution (wget | shell)' },
	{ regex: /curl\s.*\|\s*python/, severity: 'warning', message: 'Piping curl output to python — verify source is trusted' },
	// Permissions
	{ regex: /chmod\s+777\b/, severity: 'warning', message: 'Overly permissive file permissions (chmod 777)' },
	{ regex: /chmod\s+[0-7]*s/, severity: 'warning', message: 'Setting SUID/SGID bit' },
	// Network / privilege escalation
	{ regex: /--privileged/, severity: 'error', message: 'Docker privileged mode' },
	{ regex: /--network[= ]host/, severity: 'warning', message: 'Docker host network mode' },
	{ regex: /-v\s+\/:\/?/, severity: 'warning', message: 'Mounting root filesystem into container' },
	// Dangerous commands
	{ regex: /\b(shutdown|reboot|halt|poweroff)\b/, severity: 'error', message: 'System shutdown/reboot command' },
	{ regex: /\bkill\s+-9\s+1\b/, severity: 'error', message: 'Killing init process' },
	{ regex: /:\(\)\s*\{\s*:\|:&\s*\};\s*:/, severity: 'error', message: 'Fork bomb detected' },
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

export function validateSecurity(
	snakefile: string,
	dockerfile: string
): SecurityIssue[] {
	const issues: SecurityIssue[] = [];

	// Check Snakefile shell blocks
	const shellBlocks = extractShellBlocks(snakefile);
	for (const block of shellBlocks) {
		for (const pattern of SHELL_PATTERNS) {
			if (pattern.regex.test(block.content)) {
				issues.push({
					line: block.line,
					file: 'Snakefile',
					severity: pattern.severity,
					message: pattern.message
				});
			}
		}
	}

	// Also scan raw Snakefile lines for shell-like patterns outside shell: blocks
	const snakeLines = snakefile.split('\n');
	for (let i = 0; i < snakeLines.length; i++) {
		const line = snakeLines[i].trimStart();
		if (line.startsWith('shell:') || line.startsWith('#')) continue;
		// Check run: and os.system( patterns
		if (/\bos\.system\(|subprocess\.(call|run|Popen)\(/.test(line)) {
			for (const pattern of SHELL_PATTERNS) {
				if (pattern.regex.test(line)) {
					issues.push({
						line: i + 1,
						file: 'Snakefile',
						severity: pattern.severity,
						message: pattern.message
					});
				}
			}
		}
	}

	// Check Dockerfile RUN commands
	const runCommands = extractRunCommands(dockerfile);
	for (const cmd of runCommands) {
		for (const pattern of SHELL_PATTERNS) {
			if (pattern.regex.test(cmd.content)) {
				issues.push({
					line: cmd.line,
					file: 'Dockerfile',
					severity: pattern.severity,
					message: pattern.message
				});
			}
		}
	}

	return issues;
}

export function hasErrors(issues: SecurityIssue[]): boolean {
	return issues.some((i) => i.severity === 'error');
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
	// Remove SQL/DB details, stack traces, internal paths
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
