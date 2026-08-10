import { typedInvoke } from "@/lib/tauri";
import { invokeClientLocal } from "@/lib/remote/client-local-invoke";

import { UpdateChannelSchema, type UpdateChannel } from "./update-channel.schemas";

export const updateChannelApi = {
  get: (): Promise<UpdateChannel> =>
    typedInvoke("get_update_channel", {}, UpdateChannelSchema),
  set: (updateChannel: UpdateChannel): Promise<UpdateChannel> =>
    typedInvoke(
      "set_update_channel",
      { updateChannel },
      UpdateChannelSchema,
    ),
} as const;

/** Reads the release channel owned by the Mac running this UI, bypassing remote routing. */
export async function getClientUpdateChannel(): Promise<UpdateChannel> {
  return UpdateChannelSchema.parse(await invokeClientLocal("get_update_channel", {}));
}

export { UpdateChannelSchema, type UpdateChannel } from "./update-channel.schemas";
