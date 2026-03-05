import inquirer from 'inquirer';
import chalk from 'chalk';
import { execSync } from 'child_process';
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';
import { validate } from './validate.js';

const DEFAULT_REGISTRY = 'http://localhost:8090';

function compareSemver(a, b) {
  const pa = a.split('.').map(Number);
  const pb = b.split('.').map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] || 0) > (pb[i] || 0)) return 1;
    if ((pa[i] || 0) < (pb[i] || 0)) return -1;
  }
  return 0;
}

function bumpVersion(version, type) {
  const parts = version.split('.').map(Number);
  if (type === 'major') return `${parts[0] + 1}.0.0`;
  if (type === 'minor') return `${parts[0]}.${parts[1] + 1}.0`;
  return `${parts[0]}.${parts[1]}.${parts[2] + 1}`;
}

/**
 * Publish the plugin in the current directory to the AutoPipe registry.
 */
export async function publish(options = {}) {
  // 1. Validate first
  let manifest = await validate();

  const registry = (options.registry || DEFAULT_REGISTRY).replace(/\/$/, '');

  // 2. Check existing version in registry
  try {
    const checkResp = await fetch(
      `${registry}/api/plugins?name=${encodeURIComponent(manifest.name)}`
    );
    if (checkResp.ok) {
      const existing = await checkResp.json();
      if (existing && existing.version) {
        const publishedVersion = existing.version;
        const localVersion = manifest.version;
        const cmp = compareSemver(localVersion, publishedVersion);

        if (cmp <= 0) {
          console.log(
            chalk.yellow(
              `\n⚠ Registry already has ${chalk.bold(manifest.name)} v${publishedVersion}`
            )
          );
          if (cmp === 0) {
            console.log(chalk.yellow(`  Local version is the same (v${localVersion}).`));
          } else {
            console.log(
              chalk.yellow(`  Local version (v${localVersion}) is older than published.`)
            );
          }

          const nextPatch = bumpVersion(publishedVersion, 'patch');
          const nextMinor = bumpVersion(publishedVersion, 'minor');
          const nextMajor = bumpVersion(publishedVersion, 'major');

          const { action } = await inquirer.prompt([
            {
              type: 'list',
              name: 'action',
              message: 'Version must be higher than published. Choose:',
              choices: [
                { name: `patch  → v${nextPatch}`, value: 'patch' },
                { name: `minor  → v${nextMinor}`, value: 'minor' },
                { name: `major  → v${nextMajor}`, value: 'major' },
                { name: 'Cancel publish', value: 'cancel' },
              ],
            },
          ]);

          if (action === 'cancel') {
            console.log(chalk.red('\nPublish cancelled.\n'));
            return;
          }

          const newVersion = bumpVersion(publishedVersion, action);
          const manifestPath = join(process.cwd(), 'manifest.json');
          const raw = readFileSync(manifestPath, 'utf-8');
          const manifestObj = JSON.parse(raw);
          manifestObj.version = newVersion;
          writeFileSync(manifestPath, JSON.stringify(manifestObj, null, 2) + '\n');
          console.log(
            chalk.green(`\n✓ Updated manifest.json: v${localVersion} → v${newVersion}`)
          );

          manifest = await validate();
        }
      }
    }
  } catch {
    // Registry not reachable — skip version check
  }

  // 3. Get GitHub token
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

    // Convert SSH URL to HTTPS if needed, strip embedded credentials
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
