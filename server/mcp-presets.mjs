import os from 'node:os';
import path from 'node:path';
import {WORKSPACE} from './config.mjs';

// Curated, zero-configuration MCP servers that can be installed with one click.
// Credential-based servers stay in the official Registry search flow.
export const mcpPresets = () => [
  {
    id: 'filesystem',
    name: 'Filesystem',
    package: '@modelcontextprotocol/server-filesystem',
    description: 'Read and write files under your home folder.',
    args: [os.homedir()],
    category: 'Files'
  },
  {
    id: 'git',
    name: 'Git',
    package: '@modelcontextprotocol/server-git',
    description: 'Inspect branches, diffs and commit history for the workspace repository.',
    args: ['--repository', WORKSPACE],
    category: 'Developer tools'
  },
  {
    id: 'memory',
    name: 'Memory',
    package: '@modelcontextprotocol/server-memory',
    description: 'Persist entities, relations and notes in a local knowledge graph.',
    args: [],
    category: 'Memory'
  },
  {
    id: 'sequential-thinking',
    name: 'Sequential Thinking',
    package: '@modelcontextprotocol/server-sequential-thinking',
    description: 'Break difficult problems into explicit, revisable reasoning steps.',
    args: [],
    category: 'Agent runtimes'
  },
  {
    id: 'everything',
    name: 'Everything (demo)',
    package: '@modelcontextprotocol/server-everything',
    description: 'Reference server exposing prompts, resources and sample tools.',
    args: [],
    category: 'Developer tools'
  },
  {
    id: 'puppeteer',
    name: 'Puppeteer',
    package: '@modelcontextprotocol/server-puppeteer',
    description: 'Drive a local Chromium browser for navigation and screenshots.',
    args: [],
    category: 'Web'
  },
  {
    id: 'sqlite',
    name: 'SQLite',
    package: '@modelcontextprotocol/server-sqlite',
    description: 'Query and edit a local SQLite database for structured data.',
    args: ['--db-path', path.join(WORKSPACE, 'mcp.sqlite')],
    category: 'Data & analytics'
  }
];
