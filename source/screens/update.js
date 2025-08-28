// source/screens/update.jsx
import React, {useEffect, useState} from 'react';
import {Box, Text} from 'ink';
import Spinner from 'ink-spinner';
import path from 'node:path';
import fs from 'node:fs/promises';
import {existsSync} from 'node:fs';
import crypto from 'node:crypto';

const CONFIG_NAME = 'eenv.config.json';
const ENC_NAME = 'eenv.enc.json';

// Reuse same skips from Init
const SKIP_DIRS = new Set([
	'node_modules',
	'.git',
	'dist',
	'build',
	'.next',
	'.turbo',
	'.output',
	'out',
	'.cache',
]);
const MAX_DEPTH = 8;

async function listFilesRecursive(root, depth = 0) {
	const results = [];
	if (depth > MAX_DEPTH) return results;
	let entries;
	try {
		entries = await fs.readdir(root, {withFileTypes: true});
	} catch {
		return results;
	}

	for (const entry of entries) {
		const full = path.join(root, entry.name);
		if (entry.isDirectory()) {
			if (SKIP_DIRS.has(entry.name)) continue;
			results.push(...(await listFilesRecursive(full, depth + 1)));
		} else if (entry.isFile()) {
			results.push(full);
		}
	}
	return results;
}

function isEnvFile(filePath) {
	const base = path.basename(filePath);
	if (!base.startsWith('.env')) return false;
	if (base.includes('.example')) return false; // exclude examples
	return true;
}

function toExamplePath(envPath) {
	return `${envPath}.example`;
}

function stripEnvValues(content) {
	const lines = content.split(/\r?\n/);
	return lines
		.map(line => {
			if (!line.trim() || /^\s*#/.test(line)) return line;
			const eqIdx = line.indexOf('=');
			if (eqIdx === -1) return line;
			const left = line.slice(0, eqIdx);
			const right = line.slice(eqIdx + 1);
			let comment = '';
			const hashPos = right.indexOf(' #');
			if (hashPos !== -1) comment = right.slice(hashPos);
			return `${left.trim()}=${comment}`;
		})
		.join('\n');
}

async function writeExamplesFor(envPaths) {
	const written = [];
	for (const env of envPaths) {
		try {
			const raw = await fs.readFile(env, 'utf8');
			const example = stripEnvValues(raw);
			const examplePath = toExamplePath(env);
			await fs.writeFile(
				examplePath,
				example.endsWith('\n') ? example : example + '\n',
				'utf8',
			);
			written.push(examplePath);
		} catch {
			// ignore unreadable files
		}
	}
	return {count: written.length, paths: written};
}

// Simple .env parser (tolerates `export KEY=...`, quotes, spaces)
function parseEnv(content) {
	const out = {};
	const lines = content.split(/\r?\n/);
	for (let line of lines) {
		const trimmed = line.trim();
		if (!trimmed || trimmed.startsWith('#')) continue;
		// allow "export KEY=VAL"
		if (trimmed.startsWith('export ')) line = trimmed.slice(7);
		const idx = line.indexOf('=');
		if (idx === -1) continue;
		const key = line.slice(0, idx).trim();
		let val = line.slice(idx + 1).trim();

		// strip inline comment (unquoted) — cheap heuristic
		const hashPos = val.indexOf(' #');
		if (hashPos !== -1) val = val.slice(0, hashPos).trim();

		// unwrap quotes
		if (
			(val.startsWith('"') && val.endsWith('"')) ||
			(val.startsWith("'") && val.endsWith("'"))
		) {
			val = val.slice(1, -1);
		}
		out[key] = val;
	}
	return out;
}

// Key handling: stored as base64url in eenv.config.json (per Init)
function keyToBytes(keyStr) {
	// Try base64url first
	try {
		return Buffer.from(keyStr, 'base64url');
	} catch {}
	// Fallback: base64
	try {
		return Buffer.from(keyStr, 'base64');
	} catch {}
	// Last resort: hash UTF-8 to 32 bytes
	return crypto.createHash('sha256').update(keyStr, 'utf8').digest();
}

function encryptJson(obj, keyStr) {
	const keyBytes = keyToBytes(keyStr);
	if (keyBytes.length !== 32) {
		// normalize to 32 bytes if not
		const hashed = crypto.createHash('sha256').update(keyBytes).digest();
		keyBytes.set?.(hashed) ?? hashed.copy(keyBytes, 0, 0, 32);
	}
	const iv = crypto.randomBytes(12); // GCM nonce
	const cipher = crypto.createCipheriv(
		'aes-256-gcm',
		keyBytes.slice(0, 32),
		iv,
	);
	const plaintext = Buffer.from(JSON.stringify(obj));
	const enc1 = cipher.update(plaintext);
	const enc2 = cipher.final();
	const tag = cipher.getAuthTag();
	const data = Buffer.concat([enc1, enc2]);

	return {
		alg: 'AES-256-GCM',
		iv: iv.toString('base64url'),
		tag: tag.toString('base64url'),
		data: data.toString('base64url'),
		createdAt: new Date().toISOString(),
	};
}

export default function Update() {
	const [phase, setPhase] = useState('working'); // working | examples | encrypt | done | error
	const [error, setError] = useState('');
	const [report, setReport] = useState({examples: 0, encPath: ''});
	const [repoRoot, setRepoRoot] = useState('');

	useEffect(() => {
		(async () => {
			try {
				const cwd = process.cwd();
				setRepoRoot(cwd);

				// 1) find env files
				setPhase('working');
				const all = await listFilesRecursive(cwd);
				const envs = all.filter(isEnvFile);

				// 2) (re)write .example files
				setPhase('examples');
				const ex = await writeExamplesFor(envs);

				// 3) build map of env file -> {KEY:VAL}
				const map = {};
				for (const envPath of envs) {
					try {
						const raw = await fs.readFile(envPath, 'utf8');
						map[path.relative(cwd, envPath)] = parseEnv(raw);
					} catch {
						/* ignore */
					}
				}

				// 4) load key from eenv.config.json
				const cfgPath = path.join(cwd, CONFIG_NAME);
				if (!existsSync(cfgPath)) {
					throw new Error(`Missing ${CONFIG_NAME}. Run "init" first.`);
				}
				const cfgRaw = await fs.readFile(cfgPath, 'utf8');
				const cfg = cfgRaw.trim() ? JSON.parse(cfgRaw) : {};
				if (!cfg.key)
					throw new Error(`No "key" in ${CONFIG_NAME}. Run "init" to set it.`);

				// 5) encrypt and write eenv.enc.json
				setPhase('encrypt');
				const encObj = encryptJson(map, String(cfg.key));
				const encPath = path.join(cwd, ENC_NAME);
				await fs.writeFile(
					encPath,
					JSON.stringify(encObj, null, 2) + '\n',
					'utf8',
				);

				setReport({examples: ex.count, encPath});
				setPhase('done');
			} catch (e) {
				setError(e?.message ?? String(e));
				setPhase('error');
			}
		})();
	}, []);

	if (phase === 'working') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Scanning for .env files…</Text>
			</Box>
		);
	}

	if (phase === 'examples') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> (Re)creating .env*.example files…</Text>
			</Box>
		);
	}

	if (phase === 'encrypt') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Encrypting env map → {path.relative(repoRoot, ENC_NAME)}</Text>
			</Box>
		);
	}

	if (phase === 'done') {
		return (
			<Box flexDirection="column">
				<Text color="green">
					✔ Wrote {report.examples} .example file
					{report.examples === 1 ? '' : 's'}
				</Text>
				<Text color="green">
					✔ Encrypted env map → {path.relative(repoRoot, report.encPath)}
				</Text>
				<Text dimColor>
					You can commit {ENC_NAME}; it’s encrypted with your eenv.config.json
					key.
				</Text>
			</Box>
		);
	}

	return <Text color="red">✖ {error || 'Unexpected error'}</Text>;
}
