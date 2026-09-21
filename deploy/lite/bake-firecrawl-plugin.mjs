// Builder-only: retain the managed npm project, including hoisted dependencies.
import { cpSync, existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const [stateDirectory, destination] = process.argv.slice(2);
if (!stateDirectory || !destination || process.argv.length !== 4) {
  throw new Error('Usage: bake-firecrawl-plugin.mjs <installer-state> <destination>');
}

const packagePath = 'node_modules/@openclaw/firecrawl-plugin';
const projectsDirectory = join(stateDirectory, 'npm/projects');
const projects = readdirSync(projectsDirectory, { withFileTypes: true })
  .filter(entry => entry.isDirectory())
  .map(entry => join(projectsDirectory, entry.name))
  .filter(project => existsSync(join(project, packagePath, 'package.json')));
if (projects.length !== 1) {
  throw new Error(`Expected one installed Firecrawl npm project, found ${projects.length}`);
}

const project = projects[0];
const plugin = join(project, packagePath);
const metadata = JSON.parse(readFileSync(join(plugin, 'package.json'), 'utf8'));
const manifest = JSON.parse(readFileSync(join(plugin, 'openclaw.plugin.json'), 'utf8'));
if (metadata.name !== '@openclaw/firecrawl-plugin' || metadata.version !== '2026.7.1' || manifest.id !== 'firecrawl') {
  throw new Error('Installed Firecrawl plugin does not match @openclaw/firecrawl-plugin@2026.7.1 / firecrawl');
}
if (existsSync(destination)) {
  throw new Error('Baked Firecrawl destination must not already exist');
}
// Keep the OpenClaw peer link to /app and any relative dependency links intact.
cpSync(project, destination, { recursive: true, verbatimSymlinks: true, force: false, errorOnExist: true });
