import { startServer } from './server.mjs';
import { startFakeExtension, waitFor } from './fake-extension.mjs';
import { client } from './http.mjs';

export async function setup(extOpts = {}) {
  const server = await startServer();
  const api = client(server);
  let ext = null;
  try {
    if (extOpts !== false) {
      ext = await startFakeExtension({ base: server.base, token: server.token, ...extOpts });
      await waitFor(async () => (await api.health()).tabs === (extOpts.tabIds ?? [101, 102]).length, 'tabs synced');
    }
  } catch (e) {
    await server.stop();
    throw e;
  }
  const stop = async () => {
    ext?.close();
    await server.stop();
  };
  return { ...server, ...api, ext, stop };
}
