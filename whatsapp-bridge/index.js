'use strict';

const { Client, LocalAuth } = require('whatsapp-web.js');
const readline = require('readline');

const HOME = process.env.HOME || '/root';
const SESSION_PATH = `${HOME}/.automate/whatsapp-session`;

// All structured output goes to stdout as JSON lines.
// All debug/log output goes to stderr to avoid mixing with the protocol.
function emit(obj) {
  process.stdout.write(JSON.stringify(obj) + '\n');
}

function log(msg) {
  process.stderr.write(`[whatsapp-bridge] ${msg}\n`);
}

// Create the WhatsApp client with local auth for session persistence
const client = new Client({
  authStrategy: new LocalAuth({ dataPath: SESSION_PATH }),
  puppeteer: {
    headless: true,
    args: ['--no-sandbox', '--disable-setuid-sandbox'],
  },
});

// --- WhatsApp event handlers ---

client.on('qr', (qr) => {
  log('QR code received');
  emit({ type: 'qr', data: qr });
});

client.on('authenticated', () => {
  log('Authenticated (session restored or QR scanned)');
  emit({ type: 'authenticated' });
});

client.on('ready', () => {
  log('Client is ready');
  emit({ type: 'ready' });
});

client.on('message', (msg) => {
  // Only forward text messages
  if (!msg.body) return;

  const from = msg.from; // e.g. "15551234567@c.us"
  emit({
    type: 'message',
    from: from,
    body: msg.body,
    timestamp: Math.floor(msg.timestamp),
  });
});

client.on('disconnected', (reason) => {
  log(`Disconnected: ${reason}`);
  emit({ type: 'disconnected', reason: String(reason) });
});

client.on('auth_failure', (msg) => {
  log(`Auth failure: ${msg}`);
  emit({ type: 'error', message: `Auth failure: ${msg}` });
});

// --- Stdin command handling ---

const rl = readline.createInterface({ input: process.stdin });

rl.on('line', async (line) => {
  let cmd;
  try {
    cmd = JSON.parse(line);
  } catch (e) {
    log(`Invalid JSON on stdin: ${e.message}`);
    return;
  }

  if (cmd.type === 'send') {
    try {
      await client.sendMessage(cmd.to, cmd.body);
      log(`Sent message to ${cmd.to}`);
    } catch (e) {
      log(`Failed to send message: ${e.message}`);
      emit({ type: 'error', message: `Send failed: ${e.message}` });
    }
  } else if (cmd.type === 'shutdown') {
    log('Shutdown requested via stdin');
    await shutdown();
  }
});

rl.on('close', () => {
  log('Stdin closed, shutting down');
  shutdown();
});

// --- Graceful shutdown ---

let shuttingDown = false;

async function shutdown() {
  if (shuttingDown) return;
  shuttingDown = true;
  log('Shutting down...');
  try {
    await client.destroy();
  } catch (e) {
    log(`Error during client destroy: ${e.message}`);
  }
  process.exit(0);
}

process.on('SIGTERM', shutdown);
process.on('SIGINT', shutdown);

// --- Start ---

emit({ type: 'started' });
log('Starting WhatsApp client...');
client.initialize().catch((err) => {
  log(`Failed to initialize: ${err.message}`);
  emit({ type: 'error', message: `Init failed: ${err.message}` });
  process.exit(1);
});
