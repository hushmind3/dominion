#!/usr/bin/env node
// Minimal MCP stdio server used to verify the Rust MCP client.
// Not part of the runtime: this is a development fixture.
import readline from 'node:readline'

const send = (payload) => process.stdout.write(JSON.stringify(payload) + '\n')
const modern = process.argv.includes('--modern')
const emptyDiscovery = process.argv.includes('--empty-discovery')
const version = '2026-07-28'

const rl = readline.createInterface({ input: process.stdin })
rl.on('line', (line) => {
  let message
  try { message = JSON.parse(line) } catch { return }
  if (modern && message.id !== undefined) {
    if (message.params?._meta?.['io.modelcontextprotocol/protocolVersion'] !== version) {
      send({jsonrpc:'2.0',id:message.id,error:{code:-32022,message:'wrong protocol',data:{supported:[version]}}})
      return
    }
    if (message.method === 'initialize') {
      send({jsonrpc:'2.0',id:message.id,error:{code:-32601,message:'legacy initialize forbidden'}})
      return
    }
  }
  switch (message.method) {
    case 'server/discover':
      if (modern) {
        send({jsonrpc:'2.0',id:message.id,result:{supportedVersions:[version],serverInfo:{name:'echo-modern',version:'1.0.0'},capabilities:{tools:{}}}})
      } else if (emptyDiscovery) {
        send({jsonrpc:'2.0',id:message.id,result:{}})
      } else {
        send({jsonrpc:'2.0',id:message.id,error:{code:-32601,message:'Method not found'}})
      }
      break
    case 'initialize':
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: {
          protocolVersion: '2024-11-05',
          capabilities: { tools: {} },
          serverInfo: { name: 'echo-lab', version: '1.0.0' },
        },
      })
      break
    case 'notifications/initialized':
      break
    case 'tools/list':
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: {
          tools: [{
            name: 'echo',
            description: 'Echo text back with a prefix',
            inputSchema: {
              type: 'object',
              properties: { text: { type: 'string' } },
              required: ['text'],
            },
          }],
        },
      })
      break
    case 'tools/call': {
      const args = message.params?.arguments || {}
      send({
        jsonrpc: '2.0',
        id: message.id,
        result: { content: [{ type: 'text', text: (modern?'MCP_MODERN_ECHO: ':'MCP_ECHO: ') + String(args.text ?? '') }] },
      })
      break
    }
    default:
      if (message.id !== undefined) {
        send({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'Method not found' } })
      }
  }
})
