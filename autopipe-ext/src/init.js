import inquirer from 'inquirer';
import chalk from 'chalk';
import { writeFileSync, mkdirSync, existsSync } from 'fs';
import { join } from 'path';

export async function init() {
  console.log(chalk.bold('\n🔌 AutoPipe Plugin Scaffolding\n'));

  const answers = await inquirer.prompt([
    {
      type: 'input',
      name: 'name',
      message: 'Plugin name:',
      validate: (v) => (v.trim() ? true : 'Name is required'),
    },
    {
      type: 'input',
      name: 'description',
      message: 'Description:',
      default: '',
    },
    {
      type: 'input',
      name: 'extensions',
      message: 'Supported file extensions (comma-separated):',
      validate: (v) => {
        if (Array.isArray(v)) return v.length > 0 ? true : 'At least one extension is required';
        return typeof v === 'string' && v.trim() ? true : 'At least one extension is required';
      },
      filter: (v) => {
        const raw = typeof v === 'string' ? v : v.join(',');
        return raw
          .split(',')
          .map((s) => s.trim().replace(/^\./, ''))
          .filter(Boolean);
      },
    },
  ]);

  const dir = join(process.cwd(), answers.name);

  if (existsSync(dir)) {
    console.log(chalk.yellow(`\nDirectory '${answers.name}' already exists. Files will be written into it.`));
  } else {
    mkdirSync(dir, { recursive: true });
  }

  // manifest.json
  const manifest = {
    name: answers.name,
    version: '1.0.0',
    description: answers.description,
    extensions: answers.extensions,
    entry: 'index.js',
  };
  writeFileSync(join(dir, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');

  // index.js template
  const extList = answers.extensions.join(', ');
  const indexJs = `// AutoPipe Plugin: ${answers.name}
// Supported extensions: ${extList}

window.AutoPipePlugin = {
  render: function(container, fileUrl, filename) {
    container.innerHTML = '<p>Loading ' + filename + '...</p>';

    fetch(fileUrl)
      .then(function(resp) { return resp.text(); })
      .then(function(data) {
        // TODO: Replace with your custom rendering logic
        container.innerHTML = '<pre style="padding:16px;font-size:13px;overflow:auto;">' +
          data.substring(0, 10000) + '</pre>';
      })
      .catch(function(err) {
        container.innerHTML = '<p style="color:red;">Error: ' + err.message + '</p>';
      });
  },

  destroy: function() {
    // Optional: clean up event listeners, timers, etc.
  }
};
`;
  writeFileSync(join(dir, 'index.js'), indexJs);

  // README.md
  const readme = `# ${answers.name}

${answers.description || 'An AutoPipe viewer plugin.'}

## Supported Extensions

${answers.extensions.map((e) => `- \`.${e}\``).join('\n')}

## Installation

Install via the AutoPipe app:
\`\`\`
install_plugin("${answers.name}")
\`\`\`

Or manually copy this directory to your plugins folder:
- Linux/Mac: \`~/.local/share/autopipe/plugins/${answers.name}/\`
- Windows: \`%APPDATA%\\autopipe\\plugins\\${answers.name}\\\`

## Development

1. Copy the plugin to your local plugins directory
2. Run \`show_results\` in AutoPipe to test
3. Edit \`index.js\` and refresh the viewer to see changes
`;
  writeFileSync(join(dir, 'README.md'), readme);

  console.log(chalk.green(`\n✓ Plugin scaffolded in ./${answers.name}/`));
  console.log(`\n  Files created:`);
  console.log(`    manifest.json`);
  console.log(`    index.js`);
  console.log(`    README.md`);
  console.log(`\n  Next steps:`);
  console.log(`    1. Edit ${chalk.cyan('index.js')} with your rendering logic`);
  console.log(`    2. Test with ${chalk.cyan('autopipe-ext package')}`);
  console.log(`    3. Publish with ${chalk.cyan('autopipe-ext publish')}\n`);
}
