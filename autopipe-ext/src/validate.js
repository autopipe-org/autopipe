import chalk from 'chalk';
import { readFileSync, existsSync } from 'fs';
import { join } from 'path';

/**
 * Validate the plugin in the current directory.
 * Returns the parsed manifest on success, throws on failure.
 */
export async function validate() {
  const dir = process.cwd();
  const errors = [];

  // 1. Check manifest.json exists
  const manifestPath = join(dir, 'manifest.json');
  if (!existsSync(manifestPath)) {
    throw new Error('manifest.json not found in the current directory.');
  }

  // 2. Parse manifest
  let manifest;
  try {
    const raw = readFileSync(manifestPath, 'utf-8');
    manifest = JSON.parse(raw);
  } catch (e) {
    throw new Error(`manifest.json is not valid JSON: ${e.message}`);
  }

  // 3. Required fields
  if (!manifest.name || typeof manifest.name !== 'string') {
    errors.push('"name" is required and must be a string');
  }
  if (!manifest.version || typeof manifest.version !== 'string') {
    errors.push('"version" is required and must be a string');
  }
  if (!Array.isArray(manifest.extensions) || manifest.extensions.length === 0) {
    errors.push('"extensions" is required and must be a non-empty array');
  }
  if (!manifest.entry || typeof manifest.entry !== 'string') {
    errors.push('"entry" is required and must be a string');
  }

  // 4. Check entry file exists
  if (manifest.entry) {
    const entryPath = join(dir, manifest.entry);
    if (!existsSync(entryPath)) {
      errors.push(`Entry file "${manifest.entry}" not found`);
    } else {
      // 5. Check for AutoPipePlugin pattern
      const entryContent = readFileSync(entryPath, 'utf-8');
      if (!entryContent.includes('AutoPipePlugin')) {
        errors.push(
          `Entry file "${manifest.entry}" does not contain "AutoPipePlugin". ` +
            'Make sure to define window.AutoPipePlugin = { render: function(...) { ... } }'
        );
      }
    }
  }

  // 6. Check style file if specified
  if (manifest.style) {
    const stylePath = join(dir, manifest.style);
    if (!existsSync(stylePath)) {
      errors.push(`Style file "${manifest.style}" not found`);
    }
  }

  if (errors.length > 0) {
    console.log(chalk.red('\n✗ Validation failed:\n'));
    errors.forEach((e) => console.log(chalk.red(`  • ${e}`)));
    console.log('');
    throw new Error('Plugin validation failed');
  }

  console.log(chalk.green('\n✓ Plugin is valid and ready to publish'));
  console.log(`  Name: ${manifest.name}`);
  console.log(`  Version: ${manifest.version}`);
  console.log(`  Extensions: ${manifest.extensions.join(', ')}`);
  console.log(`  Entry: ${manifest.entry}`);
  if (manifest.style) console.log(`  Style: ${manifest.style}`);
  console.log('');

  return manifest;
}
