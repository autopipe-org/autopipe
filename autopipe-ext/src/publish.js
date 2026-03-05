import inquirer from 'inquirer';
import chalk from 'chalk';
import { execSync } from 'child_process';
import { validate } from './validate.js';

const DEFAULT_REGISTRY = 'http://localhost:8090';

/**
 * Publish the plugin in the current directory to the AutoPipe registry.
 */
export async function publish(options = {}) {
  // 1. Validate first
  const manifest = await validate();

  // 2. Get GitHub token
  let token = options.token || process.env.GITHUB_TOKEN;
  if (!token) {
    const answer = await inquirer.prompt([
      {
        type: 'password',
        name: 'token',
        message: 'GitHub Personal Access Token:',
        mask: '*',
        validate: (v) => (v.trim() ? true : 'Token is required'),
      },
    ]);
    token = answer.token;
  }

  // 3. Detect GitHub URL from git remote
  let githubUrl;
  try {
    const remote = execSync('git remote get-url origin', {
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    }).trim();

    // Convert SSH URL to HTTPS if needed
    if (remote.startsWith('git@github.com:')) {
      githubUrl = remote.replace('git@github.com:', 'https://github.com/').replace(/\.git$/, '');
    } else if (remote.includes('github.com')) {
      githubUrl = remote.replace(/\.git$/, '');
    }
  } catch {
    // git remote not available
  }

  if (!githubUrl) {
    throw new Error(
      'GitHub remote not found.\n' +
        'Please set up a GitHub remote first:\n' +
        '  git remote add origin https://github.com/username/my-plugin.git'
    );
  }

  const registry = (options.registry || DEFAULT_REGISTRY).replace(/\/$/, '');

  console.log(`\nPublishing ${chalk.bold(manifest.name)} v${manifest.version}...`);
  console.log(`  GitHub: ${githubUrl}`);
  console.log(`  Registry: ${registry}\n`);

  // 4. Call registry API
  const resp = await fetch(`${registry}/api/plugins/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      github_url: githubUrl,
      github_token: token,
    }),
  });

  const body = await resp.json();

  if (!resp.ok) {
    throw new Error(`Publish failed: ${body.error || resp.statusText}`);
  }

  if (body.updated) {
    console.log(chalk.green(`✓ Updated ${body.name}: v${body.previous_version} → v${body.new_version}`));
  } else {
    console.log(chalk.green(`✓ Published ${body.name} by ${body.author}`));
  }
  if (body.release_warning) {
    console.log(chalk.yellow(`  ⚠ ${body.release_warning}`));
  }
  console.log(`  Registry: ${registry}/plugins/${body.plugin_id}`);
  console.log('');
}
