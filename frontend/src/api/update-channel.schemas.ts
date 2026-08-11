import { z } from "zod";

export const UpdateChannelSchema = z.enum(["stable", "nightly"]);

export type UpdateChannel = z.infer<typeof UpdateChannelSchema>;
