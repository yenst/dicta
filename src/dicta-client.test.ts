import { describe, expect, it } from "vitest";
import { createDemoDictaClient } from "./dicta-client";

describe("demo DictaClient", () => {
  it("implements project and settings operations behind the same client contract", async () => {
    const client = createDemoDictaClient("alt_shift_r");
    const initial = await client.bootstrap();
    expect(initial.status.active_project_id).toBe("api-integration");
    expect(await client.listRecordings("api-integration")).toHaveLength(3);

    const settings = await client.setTranscriptionLanguage("nl");
    expect(settings.transcription_language).toBe("nl");
    expect((await client.getAppSettings()).transcription_language).toBe("nl");

    const created = await client.createDemoProject("New project");
    expect(created?.name).toBe("New project");
    expect((await client.bootstrap()).projects[0].id).toBe(created?.id);
  });
});
