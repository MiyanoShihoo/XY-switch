import { createInterface } from 'node:readline';
import { randomBytes } from 'node:crypto';
import { appendFileSync, existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const [directory, expectedNonce] = process.argv.slice(2);
if (!directory || !expectedNonce) throw new Error('An isolated receipt directory and nonce are required.');
const receiptPath = join(directory, 'receipt.json');
const callsPath = join(directory, 'mcp-calls.jsonl');
const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
const tool = {
  name: 'receipt',
  description: 'Add two integers and generate a new unpredictable receipt locally. Call exactly once per verification.',
  inputSchema: {
    type: 'object',
    properties: { a: { type: 'integer' }, b: { type: 'integer' }, nonce: { type: 'string' } },
    required: ['a', 'b', 'nonce'],
    additionalProperties: false,
  },
};

lines.on('line', (line) => {
  let request;
  try { request = JSON.parse(line); } catch { return; }
  if (request.id === undefined) return;
  let result;
  switch (request.method) {
    case 'initialize':
      result = { protocolVersion: request.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'xy-joint-receipt', version: '1.0.0' } };
      break;
    case 'ping':
      result = {};
      break;
    case 'tools/list':
      result = { tools: [tool] };
      break;
    case 'tools/call': {
      const args = request.params?.arguments ?? {};
      const valid = request.params?.name === 'receipt' && args.a === 17 && args.b === 25
        && args.nonce === expectedNonce && Object.keys(args).sort().join(',') === 'a,b,nonce';
      const duplicate = existsSync(receiptPath);
      appendFileSync(callsPath, JSON.stringify({ method: request.method, valid, duplicate, arguments: args }) + '\n');
      if (!valid || duplicate) {
        result = { isError: true, content: [{ type: 'text', text: 'receipt_invalid_or_repeated_call' }] };
        break;
      }
      const receipt = { a: args.a, b: args.b, nonce: args.nonce, sum: args.a + args.b, receipt: 'XY_RECEIPT_' + randomBytes(16).toString('hex') };
      writeFileSync(receiptPath, JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
      result = { content: [{ type: 'text', text: JSON.stringify(receipt) }], isError: false };
      break;
    }
    default:
      process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, error: { code: -32601, message: 'Method not found' } }) + '\n');
      return;
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, result }) + '\n');
});
