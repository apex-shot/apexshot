import { serve } from "inngest/next";
import { inngest } from "../../../../inngest/client";
import fnA from "../../../../inngest/fnA";

export const { GET, POST, PUT } = serve({
  client: inngest,
  functions: [fnA],
});
