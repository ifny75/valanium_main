import { readFile } from 'node:fs/promises';
import { join } from 'node:path';

export async function GET(request: Request) {
  const bytes = await readFile(join(process.cwd(), 'public', 'guides', 'opsec-internet-security-v2.pdf'));
  const disposition = new URL(request.url).searchParams.get('download') === '1' ? 'attachment' : 'inline';
  return new Response(new Uint8Array(bytes), { headers: {
    'Content-Type': 'application/pdf',
    'Content-Disposition': `${disposition}; filename="OPSEC_Internet_Security_Guide_v2.pdf"`,
    'Content-Length': String(bytes.length),
    'Cache-Control': 'public, max-age=3600',
    'X-Content-Type-Options': 'nosniff',
  } });
}
