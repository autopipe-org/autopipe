#!/usr/bin/env node

import { argv, exit } from 'process';
import { init } from '../src/init.js';
import { validate } from '../src/validate.js';
import { publish } from '../src/publish.js';

const command = argv[2];
const args = argv.slice(3);

function printHelp() {
  console.log(`
autopipe-ext — AutoPipe Plugin CLI

Usage:
  autopipe-ext <command> [options]

Commands:
  init       Create a new plugin project (interactive)
  package    Validate plugin structure and manifest
  publish    Publish plugin to the AutoPipe registry

Options:
  --help     Show this help message

publish options:
  --token <token>   GitHub Personal Access Token
                    (or set GITHUB_TOKEN environment variable)
  --registry <url>  Registry URL (default: http://localhost:8090)
`);
}

async function main() {
  if (!command || command === '--help' || command === '-h') {
    printHelp();
    exit(0);
  }

  try {
    switch (command) {
      case 'init':
        await init();
        break;
      case 'package':
        await validate();
        break;
      case 'publish': {
        const tokenIdx = args.indexOf('--token');
        const token = tokenIdx >= 0 ? args[tokenIdx + 1] : undefined;
        const regIdx = args.indexOf('--registry');
        const registry = regIdx >= 0 ? args[regIdx + 1] : undefined;
        await publish({ token, registry });
        break;
      }
      default:
        console.error(`Unknown command: ${command}`);
        printHelp();
        exit(1);
    }
  } catch (e) {
    console.error(e.message || e);
    exit(1);
  }
}

main();
