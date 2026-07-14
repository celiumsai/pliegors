import { processEmail } from "./handler";

export default {
  async email(message, env): Promise<void> {
    await processEmail(message, env);
  },
} satisfies ExportedHandler<Env>;
