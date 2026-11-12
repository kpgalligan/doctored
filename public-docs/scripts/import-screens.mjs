// Imports the screens probe's captures into the site's assets.
//
// usage (from public-docs/): node scripts/import-screens.mjs <out dir>
//
// <out dir> is the folder `scripts/screens-probe/run.py --out` wrote: `light/manifest.json` and
// `dark/manifest.json` with their PNGs. Every capture of both manifests is validated before
// anything is written; then each becomes `src/assets/screens/<id>.<capture>.<look>.webp` (full 2x
// size) and its entry in `src/assets/screens/screens.json` ({title, alt, width, height}, the size
// in points), which the Shot component reads. The alt text is the screen file's prose
// (`screens/<id>.md`). Never hand-edit `src/assets/screens/`.

import { readFile, writeFile, mkdir, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import sharp from 'sharp';

const LOOKS = ['light', 'dark'];
const SITE = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const SCREENS = path.join(SITE, 'screens');
const ASSETS = path.join(SITE, 'src/assets/screens');
const INDEX = path.join(ASSETS, 'screens.json');

/** The screen file's front block plus `alt` by capture name (the same rules as run.py's read_screen). */
async function readScreen(file) {
	const lines = (await readFile(file, 'utf8')).split('\n');
	if (lines[0] !== '```json screen') throw new Error(`${path.basename(file)}: no \`\`\`json screen first line`);
	const close = lines.indexOf('```', 1);
	if (close < 0) throw new Error(`${path.basename(file)}: the front block has no closing fence`);
	const screen = JSON.parse(lines.slice(1, close).join('\n'));
	const prose = lines.slice(close + 1).join('\n').trim();
	const names = screen.captures.map((capture) => capture.name);
	const tidy = (text) => text.replace(/\s+/g, ' ').trim();
	screen.alt = {};
	if (names.length === 1) {
		screen.alt[names[0]] = tidy(prose);
	} else {
		let current = null;
		const sections = {};
		for (const line of prose.split('\n')) {
			if (line.startsWith('## ')) {
				current = line.slice(3).trim();
				sections[current] = [];
			} else if (current !== null) {
				sections[current].push(line);
			}
		}
		for (const name of names) screen.alt[name] = tidy((sections[name] ?? []).join('\n'));
	}
	return screen;
}

async function main() {
	const out = process.argv[2];
	if (!out) {
		console.error('usage: node scripts/import-screens.mjs <out dir>');
		return 2;
	}
	const problems = [];
	const manifests = {};
	for (const look of LOOKS) {
		const file = path.join(out, look, 'manifest.json');
		if (!existsSync(file)) {
			console.error(`FAIL: no ${look}/manifest.json under the out dir`);
			return 1;
		}
		manifests[look] = JSON.parse(await readFile(file, 'utf8'));
		const manifest = manifests[look];
		if (manifest.look !== look) problems.push(`${look}/manifest.json says look ${manifest.look}`);
		if (manifest.scale !== 2) problems.push(`${look}/manifest.json: scale is ${manifest.scale}, not 2`);
		for (const failure of manifest.failures ?? []) problems.push(`${look}: screen failure: ${failure}`);
	}

	// Every capture by "<id>.<capture>", per look.
	const records = {};
	for (const look of LOOKS) {
		for (const record of manifests[look].captures) {
			const name = `${record.screen}.${record.capture}`;
			records[name] ??= {};
			if (records[name][look]) problems.push(`${name}: listed twice in ${look}`);
			records[name][look] = record;
		}
	}

	const screens = {};
	const pngs = {};
	for (const [name, byLook] of Object.entries(records).sort()) {
		const missing = LOOKS.filter((look) => !byLook[look]);
		if (missing.length) {
			problems.push(`${name}: missing from ${missing.join(' and ')}`);
			continue;
		}
		for (const look of LOOKS) {
			const record = byLook[look];
			if ((record.leak ?? []).length) {
				problems.push(`${name} ${look}: leak ${JSON.stringify(record.leak)}`);
			}
			if (record.error) problems.push(`${name} ${look}: error: ${record.error}`);
			if (!record.file) {
				problems.push(`${name} ${look}: no file`);
				continue;
			}
			for (const [pixels, points, axis] of [
				[record.pixel_width, record.point_width, 'width'],
				[record.pixel_height, record.point_height, 'height'],
			]) {
				if (Math.abs(pixels - 2 * points) > 1) {
					problems.push(`${name} ${look}: ${axis} ${pixels} px is not 2 × ${points} pt`);
				}
			}
			const png = path.join(out, look, record.file);
			if (!existsSync(png)) {
				problems.push(`${name} ${look}: ${record.file} is missing`);
				continue;
			}
			const meta = await sharp(png).metadata();
			if (meta.width !== record.pixel_width || meta.height !== record.pixel_height) {
				problems.push(`${name} ${look}: ${record.file} is ${meta.width}×${meta.height}, `
					+ `the manifest says ${record.pixel_width}×${record.pixel_height}`);
			}
			pngs[`${name}.${look}`] = png;
		}
		const [light, dark] = [byLook.light, byLook.dark];
		if (light.point_width !== dark.point_width || light.point_height !== dark.point_height) {
			problems.push(`${name}: light is ${light.point_width}×${light.point_height} pt, `
				+ `dark ${dark.point_width}×${dark.point_height} pt`);
		}
		const id = light.screen;
		const file = path.join(SCREENS, `${id}.md`);
		if (!existsSync(file)) {
			problems.push(`${name}: no screens/${id}.md`);
			continue;
		}
		screens[id] ??= await readScreen(file);
		if (!screens[id].alt[light.capture]) problems.push(`${name}: screens/${id}.md has no alt text for it`);
	}

	if (problems.length) {
		console.error(`FAIL: nothing written; ${problems.length} problems:`);
		for (const problem of problems) console.error(`  ${problem}`);
		return 1;
	}

	await mkdir(ASSETS, { recursive: true });
	const index = existsSync(INDEX) ? JSON.parse(await readFile(INDEX, 'utf8')) : {};
	let bytes = 0;
	let files = 0;
	for (const [name, byLook] of Object.entries(records).sort()) {
		for (const look of LOOKS) {
			const target = path.join(ASSETS, `${name}.${look}.webp`);
			await sharp(pngs[`${name}.${look}`]).webp({ quality: 88 }).toFile(target);
			bytes += (await stat(target)).size;
			files += 1;
		}
		const light = byLook.light;
		const screen = screens[light.screen];
		index[name] = {
			title: screen.title,
			alt: screen.alt[light.capture],
			width: Math.round(light.pixel_width / 2),
			height: Math.round(light.pixel_height / 2),
		};
	}
	const sorted = Object.fromEntries(Object.entries(index).sort(([a], [b]) => a.localeCompare(b)));
	await writeFile(INDEX, JSON.stringify(sorted, null, 2) + '\n');

	const skipped = new Set();
	for (const look of LOOKS) {
		for (const entry of manifests[look].skipped ?? []) skipped.add(`${entry.screen} (${entry.reason})`);
	}
	console.log(`imported ${files} WebP files (${(bytes / 1e6).toFixed(2)} MB) for `
		+ `${Object.keys(records).length} captures into src/assets/screens/`);
	console.log(skipped.size ? `skipped screens: ${[...skipped].join('; ')}` : 'skipped screens: none');
	return 0;
}

process.exitCode = await main();
