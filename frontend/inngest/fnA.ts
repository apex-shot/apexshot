import { inngest } from "./client";

// Example Inngest function
export default inngest.createFunction(
  { id: "example-function" },
  { event: "test/event" },
  async ({ event, step }) => {
    console.log("Received event:", event);

    // Your function logic here
    await step.run("process-event", async () => {
      console.log("Processing event data:", event.data);
      return { success: true };
    });

    return { message: "Function executed successfully" };
  }
);
