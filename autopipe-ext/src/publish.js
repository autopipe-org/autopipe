import inquirer from 'inquirer';
import chalk from 'chalk';
import { execSync } from 'child_process';
import { validate } from './validate.js';

const DEFAULT_REGISTRY = 'https://hub.autopipe.org';

/**
 * Publish the plugin in the current directory to the AutoPipe registry.
 *
 * Version handling: GitHub repo's manifest.json is the single source of truth.
 * The user is responsible for setting the correct version in manifest.json and
 * pushing to GitHub before running this command. The server validates that the
 * version is higher than the previously published version (same name + same
 * author). If not, the server returns 409 and the user must update manifest,
 * push, and retry.
 */
export async function publish(options = {}) {
  const manifest = await validate();
  const registry = (options.registry || DEFAULT_REGISTRY).replace(/\/$/, '');

  // Get GitHub token
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

  // Detect GitHub URL from git remote
  let githubUrl;
  try {
    const remote = execSync('git remote get-url origin', {
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    }).trim();

    if (remote.startsWith('git@github.com:')) {
      githubUrl = remote.replace('git@github.com:', 'https://github.com/').replace(/\.git$/, '');
    } else if (remote.includes('github.com')) {
      githubUrl = remote.replace(/\.git$/, '').replace(/\/\/[^@]+@/, '//');
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

  console.log(`\nPublishing ${chalk.bold(manifest.name)} v${manifest.version}...`);
  console.log(`  GitHub: ${githubUrl}`);
  console.log(`  Registry: ${registry}\n`);

  // Call registry API
  const resp = await fetch(`${registry}/api/plugins/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      github_url: githubUrl,
      github_token: token,
    }),
  });

  const body = await resp.json();

  // Version conflict — manifest version is not higher than the previously published one
  if (resp.status === 409 && body.previous_version) {
    console.log(chalk.red(`\n✗ ${body.error || 'Version conflict'}`));
    console.log(
      chalk.yellow(
        `\n  Currently published: v${body.previous_version}` +
        `\n  Your manifest:       v${manifest.version}`
      )
    );
    console.log(
      `\n  Update ${chalk.cyan('manifest.json')} to a version higher than ` +
      `v${body.previous_version}, then:\n` +
      `    git add manifest.json\n` +
      `    git commit -m "bump version"\n` +
      `    git push\n` +
      `    autopipe-ext publish\n`
    );
    return;
  }

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
