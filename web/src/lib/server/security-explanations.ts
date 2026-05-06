// English explanations for hard-layer error patterns. The publish API uses
// these to give the publisher a clear reason when a pattern blocks them.
// The MCP client renders the explanation in the chat so the user understands
// why their pipeline was rejected and what they need to change.

export interface PatternExplanation {
	short: string;
	detail: string;
}

export const PATTERN_EXPLANATIONS: Record<string, PatternExplanation> = {
	'rm-rf-recursive': {
		short: 'Recursive force delete (rm -rf)',
		detail:
			'Removes a directory tree without confirmation. If the path is computed at runtime, this can wipe data outside the pipeline workspace.'
	},
	mkfs: {
		short: 'Disk formatting (mkfs)',
		detail:
			'Reformats a filesystem and erases everything on it. This should never run inside a pipeline rule.'
	},
	'dd-write': {
		short: 'Raw disk write (dd if=...)',
		detail:
			'Writes raw bytes to a device or file at the block level. Misuse can corrupt disks or partitions.'
	},
	'dev-block-write': {
		short: 'Writes to /dev/sd*',
		detail:
			'Writes directly to a block device. This bypasses the filesystem and can destroy data on the host machine.'
	},
	'curl-pipe-shell': {
		short: 'Remote code execution (curl | shell)',
		detail:
			'Downloads a script over the network and executes it without inspection. The remote source can change at any time and inject arbitrary commands.'
	},
	'wget-pipe-shell': {
		short: 'Remote code execution (wget | shell)',
		detail:
			'Same risk as curl | shell — the downloaded payload runs immediately, with no chance to verify what it does.'
	},
	'docker-privileged': {
		short: 'Docker --privileged mode',
		detail:
			'Disables most container isolation: the container gets nearly full host access. Only use this when the pipeline has no alternative.'
	},
	'system-shutdown': {
		short: 'System shutdown/reboot/halt/poweroff',
		detail:
			'Powers off or reboots the host machine. Should never appear in a pipeline.'
	},
	'kill-init': {
		short: 'Killing init process (kill -9 1)',
		detail:
			'Sends SIGKILL to PID 1, which can panic the kernel or terminate the entire container. There is no legitimate pipeline use.'
	},
	'fork-bomb': {
		short: 'Fork bomb',
		detail:
			'Self-replicates processes until the system runs out of resources. There is no legitimate use; this denies service to the host.'
	}
};

export function explainPattern(patternId: string): PatternExplanation {
	return (
		PATTERN_EXPLANATIONS[patternId] || {
			short: patternId,
			detail: 'No detailed explanation available for this pattern.'
		}
	);
}
