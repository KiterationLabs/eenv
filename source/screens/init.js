// source/screens/init.jsx
import React, {useEffect, useState} from 'react';
import {Box, Text} from 'ink';
import Spinner from 'ink-spinner';
import TextInput from 'ink-text-input';
import path from 'node:path';
import fs from 'node:fs/promises';
import {existsSync} from 'node:fs';
import crypto from 'node:crypto';
import Apply from './apply.js'; // ⟵ make sure this path matches your project

const CONFIG_NAME = 'eenv.config.json';
const ENC_NAME = 'eenv.enc.json';
const IGNORE_BLOCK_START = '# Added by eenv';
const IGNORE_BLOCK_END = ''; // blank line after block

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
	if (base.includes('.example')) return false;
	return true;
}

async function ensureGitignoreHas(envPaths, repoRoot) {
	const giPath = path.join(repoRoot, '.gitignore');
	let content = '';
	if (existsSync(giPath)) content = await fs.readFile(giPath, 'utf8');

	const rels = envPaths
		.map(p => path.relative(repoRoot, p))
		.map(p => (path.sep === '\\' ? p.replace(/\\/g, '/') : p));

	const existingSet = new Set(
		content
			.split('\n')
			.map(l => l.trim())
			.filter(Boolean),
	);
	const toAdd = [];
	for (const rel of rels) if (!existingSet.has(rel)) toAdd.push(rel);

	if (toAdd.length === 0) return {updated: false, added: []};

	const block = [IGNORE_BLOCK_START, ...toAdd, IGNORE_BLOCK_END, ''].join('\n');
	const needsNL = content.length > 0 && !content.endsWith('\n');
	const next = (content || '') + (needsNL ? '\n' : '') + block;
	await fs.writeFile(giPath, next, 'utf8');
	return {updated: true, added: toAdd};
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
			/* ignore */
		}
	}
	return {count: written.length, paths: written};
}

function generateKey() {
	return crypto.randomBytes(32).toString('base64url');
}

function keyToBytes(keyStr) {
	try {
		return Buffer.from(keyStr, 'base64url');
	} catch {}
	try {
		return Buffer.from(keyStr, 'base64');
	} catch {}
	return crypto.createHash('sha256').update(keyStr, 'utf8').digest(); // 32 bytes
}

function tryDecryptEnc(encObj, keyStr) {
	const key = keyToBytes(keyStr);
	const k =
		key.length === 32 ? key : crypto.createHash('sha256').update(key).digest();
	const iv = Buffer.from(encObj.iv, 'base64url');
	const tag = Buffer.from(encObj.tag, 'base64url');
	const data = Buffer.from(encObj.data, 'base64url');
	const decipher = crypto.createDecipheriv('aes-256-gcm', k, iv);
	decipher.setAuthTag(tag);
	const dec = Buffer.concat([decipher.update(data), decipher.final()]);
	// Parse to confirm it's valid JSON; we discard it now—Apply will do the real work.
	JSON.parse(dec.toString('utf8'));
	return true;
}

export default function Init({name}) {
	// working | ask | askPass | saving | gitignore | examples | apply | done | error
	const [phase, setPhase] = useState('working');
	const [configPath, setConfigPath] = useState('');
	const [repoRoot, setRepoRoot] = useState('');
	const [key, setKey] = useState('');
	const [pass, setPass] = useState('');
	const [error, setError] = useState('');
	const [gitignoreReport, setGitignoreReport] = useState({
		updated: false,
		added: [],
	});
	const [examplesReport, setExamplesReport] = useState({count: 0, paths: []});
	const [shouldRunApply, setShouldRunApply] = useState(false);

	useEffect(() => {
		(async () => {
			try {
				const cwd = process.cwd();
				setRepoRoot(cwd);
				const cfgPath = path.join(cwd, CONFIG_NAME);
				const encPath = path.join(cwd, ENC_NAME);
				setConfigPath(cfgPath);

				// Does config exist with key?
				let cfg = {};
				if (existsSync(cfgPath)) {
					try {
						const raw = await fs.readFile(cfgPath, 'utf8');
						cfg = raw.trim() ? JSON.parse(raw) : {};
						if (cfg?.key) setKey(String(cfg.key));
					} catch {
						/* ignore */
					}
				}

				const hasKey = Boolean(cfg?.key);
				const hasEnc = existsSync(encPath);

				if (hasEnc && !hasKey) {
					// We have an encrypted map but no key stored: ask user for pass/key to unlock
					setPhase('askPass');
				} else {
					// Normal flow: ask for key (pre-filled if present)
					// If cfg missing, create empty file so path exists
					if (!existsSync(cfgPath)) {
						await fs.writeFile(cfgPath, JSON.stringify({}, null, 2) + '\n', {
							mode: 0o600,
						});
					}
					setPhase('ask');
				}
			} catch (e) {
				setError(e?.message ?? String(e));
				setPhase('error');
			}
		})();
	}, []);

	// Submit when we need to create/update config key
	const onSubmitKey = async () => {
		setPhase('saving');
		try {
			const finalKey =
				key && key.trim().length > 0 ? key.trim() : generateKey();

			// merge & write config
			let base = {};
			try {
				const raw = await fs.readFile(configPath, 'utf8');
				base = raw.trim() ? JSON.parse(raw) : {};
			} catch {}
			const next = {...base, key: finalKey};
			await fs.writeFile(
				configPath,
				JSON.stringify(next, null, 2) + '\n',
				'utf8',
			);
			try {
				await fs.chmod(configPath, 0o600);
			} catch {}

			// scan for envs and post-steps
			setPhase('gitignore');
			const all = await listFilesRecursive(repoRoot);
			const envs = all.filter(isEnvFile);

			const gi = await ensureGitignoreHas(
				[...envs, path.join(repoRoot, CONFIG_NAME)],
				repoRoot,
			);
			setGitignoreReport(gi);

			setPhase('examples');
			const ex = await writeExamplesFor(envs);
			setExamplesReport(ex);

			setPhase('done');
		} catch (e) {
			setError(e?.message ?? String(e));
			setPhase('error');
		}
	};

	// Submit when we’re adopting an existing enc: verify pass → write key → run Apply
	const onSubmitPass = async () => {
		setPhase('saving');
		try {
			const encPath = path.join(repoRoot, ENC_NAME);
			const rawEnc = await fs.readFile(encPath, 'utf8');
			let encObj;
			try {
				encObj = JSON.parse(rawEnc);
			} catch {
				throw new Error(`${ENC_NAME} is not valid JSON`);
			}

			// Verify passphrase/key by attempting decryption
			tryDecryptEnc(encObj, pass.trim());

			// Save key in config
			const next = {key: pass.trim()};
			await fs.writeFile(
				configPath,
				JSON.stringify(next, null, 2) + '\n',
				'utf8',
			);
			try {
				await fs.chmod(configPath, 0o600);
			} catch {}
			// Ensure .gitignore and .example files too (same as onSubmitKey)
			setPhase('gitignore');
			const all = await listFilesRecursive(repoRoot);
			const envs = all.filter(isEnvFile);
			const gi = await ensureGitignoreHas(
				[...envs, path.join(repoRoot, CONFIG_NAME)],
				repoRoot,
			);
			setGitignoreReport(gi);

			setPhase('examples');
			const ex = await writeExamplesFor(envs);
			setExamplesReport(ex);

			// Now materialize .env files from eenv.enc.json
			setShouldRunApply(true);
			setPhase('apply');
		} catch (e) {
			setError('Invalid passphrase/key for eenv.enc.json');
			setPhase('askPass'); // go back to prompt
		}
	};

	// ---------- UI states ----------
	if (phase === 'working') {
		return (
			<Box>
				<Text color="cyan">
					<Spinner type="dots" />
				</Text>
				<Text> Preparing init for {name}…</Text>
			</Box>
		);
	}

	if (phase === 'ask') {
		return (
			<Box flexDirection="column">
				<Text>
					Config file: <Text color="cyan">{configPath}</Text>
				</Text>
				<Text dimColor>
					Enter encryption key (leave blank to auto-generate):
				</Text>
				<Box>
					<Text color="cyanBright">› </Text>
					<TextInput
						value={key}
						onChange={setKey}
						onSubmit={onSubmitKey}
						placeholder="your-secret-key (or leave blank)"
						mask="*"
					/>
				</Box>
				<Text dimColor>Press Enter to save</Text>
			</Box>
		);
	}

	if (phase === 'askPass') {
		return (
			<Box flexDirection="column">
				<Text>
					Found <Text color="cyan">{ENC_NAME}</Text> but no stored key.
				</Text>
				<Text dimColor>
					Enter passphrase/key to unlock and save it to {CONFIG_NAME}:
				</Text>
				<Box>
					<Text color="cyanBright">› </Text>
					<TextInput
						value={pass}
						onChange={setPass}
						onSubmit={onSubmitPass}
						placeholder="enter passphrase/key"
						mask="*"
					/>
				</Box>
				{error && <Text color="red">✖ {error}</Text>}
				<Text dimColor>Press Enter to continue</Text>
			</Box>
		);
	}

	if (phase === 'saving') {
		return (
			<Box>
				<Text color="cyan">
					<Spinner type="dots" />
				</Text>
				<Text> Saving configuration…</Text>
			</Box>
		);
	}

	if (phase === 'gitignore') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Updating .gitignore…</Text>
			</Box>
		);
	}

	if (phase === 'examples') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Creating .env*.example files…</Text>
			</Box>
		);
	}

	if (phase === 'apply') {
		// Hand off to Apply screen (overwrite existing files by default)
		return <Apply force={true} />;
	}

	if (phase === 'done') {
		return (
			<Box flexDirection="column">
				<Text color="green">✔ Saved key to {configPath}</Text>
				{gitignoreReport.updated ? (
					<Text color="green">
						✔ Updated .gitignore with {gitignoreReport.added.length} entr
						{gitignoreReport.added.length === 1 ? 'y' : 'ies'}
					</Text>
				) : (
					<Text dimColor>ℹ No new .env* entries needed in .gitignore</Text>
				)}
				{examplesReport.count > 0 ? (
					<>
						<Text color="green">
							✔ Wrote {examplesReport.count} .example file
							{examplesReport.count === 1 ? '' : 's'}
						</Text>
						{examplesReport.paths.slice(0, 8).map((p, i) => (
							<Text key={i} dimColor>
								{' '}
								• {path.relative(repoRoot, p)}
							</Text>
						))}
						{examplesReport.count > 8 && (
							<Text dimColor> • …and {examplesReport.count - 8} more</Text>
						)}
					</>
				) : (
					<Text dimColor>ℹ No .env files found to mirror as .example</Text>
				)}
			</Box>
		);
	}

	return <Text color="red">✖ {error || 'Unexpected error'}</Text>;
}
