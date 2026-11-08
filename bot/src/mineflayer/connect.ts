// Join the server and load the plugins (spec §22). Glue only.

import { once } from "node:events";

import mineflayer, { type Bot } from "mineflayer";
import armorManager from "mineflayer-armor-manager";
import { plugin as collectBlock } from "mineflayer-collectblock";
import pathfinderPkg from "mineflayer-pathfinder";
import { plugin as pvp } from "mineflayer-pvp";

const { pathfinder, Movements } = pathfinderPkg;

/** Where to join. The server is private and offline-mode (spec §22). */
export interface ServerConfig {
  host: string;
  port: number;
  username: string;
  /** Pinned game version (#29). */
  version: string;
}

export const DEFAULT_SERVER: ServerConfig = {
  host: "localhost",
  port: 25565,
  username: "curious",
  version: "1.21.11",
};

export async function connect(cfg: ServerConfig): Promise<Bot> {
  const bot = mineflayer.createBot({
    host: cfg.host,
    port: cfg.port,
    username: cfg.username,
    version: cfg.version,
    auth: "offline",
  });
  bot.loadPlugin(pathfinder);
  bot.loadPlugin(collectBlock);
  bot.loadPlugin(pvp);
  bot.loadPlugin(armorManager);
  await once(bot, "spawn");
  const moves = new Movements(bot);
  moves.canDig = true;
  moves.allowSprinting = true;
  bot.pathfinder.setMovements(moves);
  return bot;
}
