# EENV
**Encrypted Env Manager**

EENV keeps secrets safe and dev-friendly:

- **Encrypts** `.env* → .env*.enc` with XChaCha20-Poly1305 (single shared key).
- **De/encrypts on demand** so teammates can pull encrypted files and decrypt locally with the same key.
- **Blocks secret leaks** by refusing commits that include raw `.env*` files.
- **Generates** `.env*.example` skeletons automatically.
- **Manages** a **pre-commit** hook so all of the above runs for you.

## NOTE
**Active Development**  
EENV is still under active development. 

I’ll do my best to minimize breaking changes, and when they are unavoidable, I’ll announce them ahead of time and specify the release where they’ll occur.  

**Feedback**  
Suggestions, issues, and ideas for improvements are very welcome! Please open an issue or discussion in the repo if you’d like to contribute.

## Install
```bash
cargo install eenv
```

> The binary is `eenv`.

## Quick Setup
In a repo that has `.env` files:
```bash
# one-time setup: installs hook, fixes .gitignore, ensures config, generates examples, encrypts
eenv init
```

First time on a new machine (only `.enc` files exist), run `eenv init` and enter the shared key to decrypt.

---

## Commands (overview)

### `eenv init`
- Prints repo state.
- If `.env*.enc` exist:
  - With a valid `eenv.config.json`, **decrypts** to plaintext **without clobbering** existing files.
  - If config is missing/invalid, **prompts for key** and bootstraps it.
- If real `.env*` exist:
  - **Generates** `.env*.example`.
  - **Aligns** `.gitignore` (keeps examples & `.enc`, ignores real `.env*` and `eenv.config.json`).
  - **Encrypts** `.env* → .env*.enc`.

### `eenv pre-commit [--write]`
- Always **blocks** staging raw `.env*` (except `*.example` / `*.enc`).
- With `--write`:
  - **Generates/updates** `.env*.example`.
  - **Fixes** `.gitignore` if needed.
  - **Ensures** `eenv.config.json` exists/valid.
  - **Encrypts** `.env* → .env*.enc` and `git add`s produced artifacts.

### `eenv hook install [--force]`
- Installs the **pre-commit** hook (respects `git config core.hooksPath`).
- `--force` will overwrite a non-EENV hook (backs it up first).

### `eenv hook uninstall [--force]`
- Removes the EENV pre-commit hook.
- `--force` removes the hook file even if it didn’t come from EENV.

*(There’s also a small demo `greet` command.)*

---

## Typical Flows

### New project with plaintext env files
```bash
eenv init
git add .env*.enc .env*.example .gitignore
git commit -m "Set up EENV"
```

### Teammate / CI on a fresh clone
```bash
eenv init            # enter the shared key when prompted
# now you have decrypted .env files locally (without clobbering existing ones)
```

### Day-to-day committing
- Stage your changes as usual.
- The **pre-commit** hook runs:
  - Refuses raw `.env*` in the index.
  - If you want auto-fixes and fresh encryption:
    - Run `eenv pre-commit --write` (or rely on the hook if you configured it to call with `--write`).

---

## Key & Security Notes
- The shared key lives in `eenv.config.json` (ignored by git).  
  A stable 32-byte key is derived using **BLAKE3**; files are encrypted with **XChaCha20-Poly1305** using a random per-file nonce.
- To rotate the key: update `eenv.config.json` with the new key and run `eenv pre-commit --write`.

---

## Uninstall
```bash
# remove the hook
eenv hook uninstall           # or: eenv hook uninstall --force
```
*(This does not delete your `.enc` files or config.)*

---

## FAQ
- **Git GUI/clients (e.g., GitHub Desktop)?**  
  If they respect Git hooks (most do when the hook files are in the repo’s hooks path), the EENV pre-commit will run. EENV installs into whatever `git rev-parse --git-path hooks` returns, so it works with custom `core.hooksPath` too.

- **“unrecognized subcommand 'PreCommit'”**  
  Use kebab-case: `eenv pre-commit` (Clap maps `PreCommit` → `pre-commit`).
