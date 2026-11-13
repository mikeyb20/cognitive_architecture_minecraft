// Start the bot and wait for the runner (spec §22). Glue only: the link
// logic is in link/ and unit-tested there. Settings come from the runner's
// hello; only where to connect comes from the environment:
//   MC_HOST, MC_PORT, MC_USER   the Minecraft server (default localhost:25565, "curious")
//   LINK_PORT                   where the runner connects (default 7878, localhost only)

import { BotServer } from "../link/server.js";
import { listen } from "../link/socket.js";
import { MineflayerBody } from "./body.js";
import { DEFAULT_SERVER, connect } from "./connect.js";
import { Perception } from "./perception.js";

/** How far the bot looks, in blocks. */
const VIEW_RADIUS = 32;
const LINK_PORT = 7878;

const env = process.env;
const bot = await connect({
  host: env.MC_HOST ?? DEFAULT_SERVER.host,
  port: Number(env.MC_PORT ?? DEFAULT_SERVER.port),
  username: env.MC_USER ?? DEFAULT_SERVER.username,
  version: DEFAULT_SERVER.version,
});
const perception = new Perception(bot, VIEW_RADIUS);
const isFood = (name: string) => bot.registry.foodsByName[name] !== undefined;
const link = new BotServer({
  names: () => [...Object.keys(bot.registry.blocksByName), ...Object.keys(bot.registry.itemsByName)],
  snapshot: () => perception.snapshot(),
  body: new MineflayerBody(bot),
  say: (text) => bot.chat(text),
});
bot.on("health", () => {
  link.watch(perception.body(isFood));
});
bot.on("death", () => link.reset());
bot.on("end", (reason) => {
  console.error(`disconnected: ${reason}`);
  process.exit(1);
});
const port = Number(env.LINK_PORT ?? LINK_PORT);
await listen(port, "127.0.0.1", (line) => link.answer(line));
console.log(`bot ${bot.username} ready; runner link on 127.0.0.1:${port}`);
