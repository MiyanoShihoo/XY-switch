import { readFile } from 'node:fs/promises';
import { packageInput } from './lib.mjs';
const [root, platform] = process.argv.slice(2);
const { version } = JSON.parse(await readFile(new URL('../../package.json', import.meta.url), 'utf8'));
console.log(JSON.stringify(await packageInput(root, platform, version), null, 2));
