#!/usr/bin/env node
import { readdirSync, statSync } from 'fs';
import { join } from 'path';
import process from 'process';

const IGNORES = new Set(['node_modules', '.git', '.next', 'dist', 'build']);

function findEnvFiles(dir = '.') {
	let results = [];
	const entries = readdirSync(dir);
	for (const name of entries) {
		const full = join(dir, name);
		let st;
		try {
			st = statSync(full);
		} catch {
			continue;
		}
		if (st.isDirectory()) {
			if (!IGNORES.has(name)) results = results.concat(findEnvFiles(full));
		} else if (name.startsWith('.env')) {
			results.push(full);
		}
	}
	return results;
}

const target = process.argv[2] || '.';
const files = findEnvFiles(target);

if (files.length) {
	for (const f of files) console.log(f);
	process.exit(0);
} else {
	console.log('No .env* files found.');
	process.exit(1);
}
