import { error } from "@sveltejs/kit";
import { get_config } from "$lib/apiService.ts";
import type { PageServerLoad } from "./$types";
import { type PropsConfig } from "../lib/models";

export const ssr = true;

export const load: PageServerLoad = async () => {
  const config = await get_config();

  console.log(`config ${JSON.stringify(config, null, 4)}`);

  if (config) {
    const props: PropsConfig = {
      config: config,
    };
    return props;
  }

  error(404, "Not found");
};
