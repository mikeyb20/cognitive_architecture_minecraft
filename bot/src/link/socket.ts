// The runner connects here over TCP (spec §22): JSON lines in, JSON lines
// out, answered strictly in order. Only localhost by default.

import { type Server, type Socket, createServer } from "node:net";

/** Split a byte stream into lines; keeps a partial line until it ends. */
export class Lines {
  #rest = "";

  push(chunk: string): string[] {
    const parts = (this.#rest + chunk).split("\n");
    this.#rest = parts.pop() as string;
    return parts.map((l) => l.replace(/\r$/, "")).filter((l) => l.trim() !== "");
  }
}

/** Serve `answer` on one connection: each reply waits for the one before. */
export function serveSocket(socket: Socket, answer: (line: string) => Promise<string>): void {
  const lines = new Lines();
  // Decodes UTF-8 split across chunks.
  const text = new TextDecoder();
  let queue: Promise<void> = Promise.resolve();
  socket.on("data", (chunk: Buffer) => {
    for (const line of lines.push(text.decode(chunk, { stream: true }))) {
      queue = queue.then(async () => {
        socket.write(`${await answer(line)}\n`);
      });
    }
  });
  // A dropped runner, or a reply written after it left: Node closes the
  // socket after "error"; listening stops the error from crashing the bot.
  socket.on("error", () => {});
}

/** Listen on `host:port`; resolves once listening. */
export async function listen(port: number, host: string, answer: (line: string) => Promise<string>): Promise<Server> {
  const server = createServer((s) => serveSocket(s, answer));
  await new Promise<void>((resolve) => server.listen(port, host, resolve));
  return server;
}
