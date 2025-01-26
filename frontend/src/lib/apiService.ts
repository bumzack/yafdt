import type { Config, DuplicateFiles } from "./models";

import { env } from "$env/dynamic/public";

export const get_config = async (): Promise<Config> => {
  const server = env.PUBLIC_BACKEND_URL;
  try {
    const url = `${server}/api/config`;
    console.log(`url ${url}`);

    const response = await fetch(url, {
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json",
      },
    });

    if (response.ok) {
      return await response.json();
    } else {
      const error = new Error("error loading config");
      return Promise.reject(error);
    }
  } catch (e) {
    console.info(`error getting confog data ${e}`);
  }
  return Promise.reject("something gone wrong");
};

export const get_duplicates = async (
  config: Config,
): Promise<DuplicateFiles[]> => {
  const server = env.PUBLIC_BACKEND_URL;
  const payload = JSON.stringify(config);
  try {
    const url = `${server}/api/duplicates`;
    console.log(`url ${url}`);

    const response = await fetch(url, {
      headers: {
        "Content-Type": "application/json",
        Accept: "application/json, text/plain, */*",
      },
      body: payload,
      method: "POST",
    });

    if (response.ok) {
      return await response.json();
    } else {
      const error = new Error("error loading duplicates");
      return Promise.reject(error);
    }
  } catch (e) {
    console.info(`error getting duplicates data ${e}`);
  }
  return [];
};
