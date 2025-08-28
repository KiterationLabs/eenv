import React, {useEffect, useState} from 'react';
import {Box, Text} from 'ink';
import Spinner from 'ink-spinner';
import path from 'node:path';
import fs from 'node:fs/promises';
import {existsSync} from 'node:fs';
import crypto from 'node:crypto';

const CONFIG_NAME = 'eenv.config.json';
const ENC_NAME = 'eenv.enc.json';

function keyToBytes(keyStr) {
	// prefer base64url, then base64, else hash to 32 bytes
	try {
		return Buffer.from(keyStr, 'base64url');
	} catch {}
	try {
		return Buffer.from(keyStr, 'base64');
	} catch {}
	return crypto.createHash('sha256').update(keyStr, 'utf8').digest();
}

function decryptJson(encObj, keyStr) {
	const keyBytes = keyToBytes(keyStr);
	const key =
		keyBytes.length === 32
			? keyBytes
			: crypto.createHash('sha256').update(keyBytes).digest();
	const iv = Buffer.from(encObj.iv, 'base64url');
	const tag = Buffer.from(encObj.tag, 'base64url');
	const data = Buffer.from(encObj.data, 'base64url');

	const decipher = crypto.createDecipheriv('aes-256-gcm', key, iv);
	decipher.setAuthTag(tag);
	const dec1 = decipher.update(data);
	const dec2 = decipher.final();
	const plaintext = Buffer.concat([dec1, dec2]).toString('utf8');
	return JSON.parse(plaintext);
}

// keep it simple: key=value (quote values that clearly need quoting)
function needsQuotes(v) {
	return /[\s#"']/g.test(v) || v.includes('=');
}
function serializeEnv(obj) {
	const lines = [];
	for (const [k, v] of Object.entries(obj)) {
		const val = v == null ? '' : String(v);
		lines.push(`${k}=${needsQuotes(val) ? JSON.stringify(val) : val}`);
	}
	return lines.join('\n') + '\n';
}

export default function Apply({force = false}) {
	const [phase, setPhase] = useState('working'); // working | decrypt | write | done | error
	const [error, setError] = useState('');
	const [written, setWritten] = useState(0);
	const [skipped, setSkipped] = useState(0);
	const [repoRoot, setRepoRoot] = useState('');

	useEffect(() => {
		(async () => {
			try {
				const cwd = process.cwd();
				setRepoRoot(cwd);

				// 1) read key
				const cfgPath = path.join(cwd, CONFIG_NAME);
				if (!existsSync(cfgPath))
					throw new Error(`Missing ${CONFIG_NAME}. Run "init" first.`);
				const cfgRaw = await fs.readFile(cfgPath, 'utf8');
				const cfg = cfgRaw.trim() ? JSON.parse(cfgRaw) : {};
				if (!cfg.key)
					throw new Error(`No "key" in ${CONFIG_NAME}. Run "init" to set it.`);

				// 2) read encrypted blob
				const encPath = path.join(cwd, ENC_NAME);
				if (!existsSync(encPath))
					throw new Error(`Missing ${ENC_NAME}. Run "update" to generate it.`);
				const encRaw = await fs.readFile(encPath, 'utf8');
				let encObj;
				try {
					encObj = JSON.parse(encRaw);
				} catch {
					throw new Error(`${ENC_NAME} is not valid JSON`);
				}

				setPhase('decrypt');
				const envMap = decryptJson(encObj, String(cfg.key)); // { "relative/path/.env": {KEY:VAL} }

				// 3) write files
				setPhase('write');
				let w = 0,
					s = 0;
				for (const [relPath, kv] of Object.entries(envMap)) {
					const abs = path.join(cwd, relPath);
					const dir = path.dirname(abs);
					await fs.mkdir(dir, {recursive: true});

					if (existsSync(abs) && !force) {
						s++;
						continue;
					}
					const content = serializeEnv(kv);
					await fs.writeFile(abs, content, 'utf8');
					w++;
				}

				setWritten(w);
				setSkipped(s);
				setPhase('done');
			} catch (e) {
				setError(e?.message ?? String(e));
				setPhase('error');
			}
		})();
	}, [force]);

	if (phase === 'working') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Reading config…</Text>
			</Box>
		);
	}
	if (phase === 'decrypt') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Decrypting env map…</Text>
			</Box>
		);
	}
	if (phase === 'write') {
		return (
			<Box>
				<Text color="yellow">
					<Spinner type="dots" />
				</Text>
				<Text> Writing .env files…</Text>
			</Box>
		);
	}
	if (phase === 'done') {
		return (
			<Box flexDirection="column">
				<Text color="green">
					✔ Wrote {written} .env file{written === 1 ? '' : 's'}
				</Text>
				{skipped > 0 && (
					<Text dimColor>
						ℹ Skipped {skipped} existing file{skipped === 1 ? '' : 's'} (use
						--force to overwrite)
					</Text>
				)}
				<Text dimColor>Root: {repoRoot}</Text>
			</Box>
		);
	}
	return <Text color="red">✖ {error || 'Unexpected error'}</Text>;
}
