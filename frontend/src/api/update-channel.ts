import { typedInvoke } from "@/lib/tauri";

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

export { UpdateChannelSchema, type UpdateChannel } from "./update-channel.schemas";
