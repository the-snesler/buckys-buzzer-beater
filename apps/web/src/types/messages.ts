import { z } from 'zod';

const PartialMessageSchema = z.discriminatedUnion("type", [
  z.object({
    PlayerList: z.array(
      z.object({
        pid: z.number(),
        name: z.string(),
        score: z.number(),
        buzzed: z.boolean(),
      })
    ),
  }),
  z.object({ BuzzEnable: z.object({}) }),
  z.object({ BuzzDisable: z.object({}) }),
  z.object({ Buzz: z.object({ pid: z.number() }) }),
  z.object({ DoHeartbeat: z.object({ hbid: z.number(), t_sent: z.number() }) }),
  z.object({ Heartbeat: z.object({ hbid: z.number() }) }),
  z.object({ GotHeartbeat: z.object({ hbid: z.number() }) }),
  z.object({
    LatencyOfHeartbeat: z.object({ hbid: z.number(), t_lat: z.number() }),
  }),
]);

export const NetworkMessageSchema = z.discriminatedUnion('type', [
  PartialMessageSchema,
  z.object({ "Witness": PartialMessageSchema }),
]);

export type NetworkMessage = z.infer<typeof NetworkMessageSchema>;
