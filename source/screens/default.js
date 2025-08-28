import React from 'react';
import {Box, Text} from 'ink';

function Section({title, children}) {
	return (
		<Box flexDirection="column" marginTop={1}>
			<Text bold color="cyan">
				{title}
			</Text>
			{children}
		</Box>
	);
}

export default function Default({error}) {
	return (
		<Box flexDirection="column">
			{error && (
				<Box marginBottom={1}>
					<Text color="red">✖ {error}</Text>
				</Box>
			)}

			<Text>
				<Text color="magentaBright">my-ink-cli</Text>{' '}
				<Text dimColor>— colorful React-style CLI</Text>
			</Text>

			<Section title="Usage">
				<Text>
					{' '}
					$ <Text color="green">my-ink-cli</Text>{' '}
					<Text dimColor>&lt;command&gt;</Text> <Text dimColor>[options]</Text>
				</Text>
			</Section>

			<Section title="Commands">
				<Text>
					{' '}
					<Text color="yellow">init</Text> Initialize something
				</Text>
				<Text>
					{' '}
					<Text color="yellow">lock</Text> Lock something
				</Text>
			</Section>

			<Section title="Options">
				<Text>
					{' '}
					<Text color="cyan">--name</Text> &lt;str&gt; Your name (used by some
					commands)
				</Text>
				<Text>
					{' '}
					<Text color="cyan">-h</Text>, <Text color="cyan">--help</Text> Show
					this help
				</Text>
			</Section>

			<Section title="Examples">
				<Text>
					{' '}
					$ <Text color="green">my-ink-cli</Text>
				</Text>
				<Text>
					{' '}
					$ <Text color="green">my-ink-cli</Text> init --name=Jane
				</Text>
				<Text>
					{' '}
					$ <Text color="green">my-ink-cli</Text> lock
				</Text>
			</Section>

			<Box marginTop={1}>
				<Text dimColor>Press Ctrl+C to exit</Text>
			</Box>
		</Box>
	);
}
