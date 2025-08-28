#!/usr/bin/env node
import React from 'react';
import {render} from 'ink';
import meow from 'meow';
import Default from './screens/default.js';
import Init from './screens/init.js';
import Update from './screens/update.js';
import Apply from './screens/apply.js';

const cli = meow(
	`
Usage
  $ my-ink-cli <command> [options]

Commands
  init            Initialize something
  update          Update something
	apply           Apply something

Options
  --name <str>    Your name (used by some commands)
  -h, --help      Show help

Examples
  $ my-ink-cli
  $ my-ink-cli init
  $ my-ink-cli update
	$ my-ink-cli apply
`,
	{
		importMeta: import.meta,
		flags: {
			help: {type: 'boolean', alias: 'h'},
		},
	},
);

const [cmd] = cli.input;

// No command or explicit --help → render the styled usage screen
if (!cmd || cli.flags.help) {
	render(<Default />);
} else {
	switch (cmd) {
		case 'init':
			render(<Init name={cli.flags.name} />);
			break;
		case 'update':
			render(<Update />);
			break;
		case 'apply':
			render(<Apply />);
			break;
		default:
			// Unknown command: show usage with an error banner
			render(<Default error={`Unknown command: ${cmd}`} />);
			break;
	}
}
