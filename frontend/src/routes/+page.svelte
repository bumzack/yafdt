<script lang="ts">
  import { get_duplicates } from "$lib/apiService";
  import LoadingSpinner from "$lib/components/LoadingSpinner.svelte";
  import { type AFileRequest, type DuplicateFilesRequest } from "$lib/models";
  import type { PageData } from "./$types";
  import Number from "$lib/components/Number.svelte";
  import Path from "$lib/components/Path.svelte";

  let { data }: { data: PageData } = $props();
  let config = $state(data.config);

  let running = $state(false);
  let duplicates: Array<DuplicateFilesRequest> = $state([]);
  let has_data = $derived(duplicates.length > 0);

  $effect(() => {
    console.log(`got a config ${JSON.stringify(config, null, 4)}`);
  });

  const start_search = () => {
    console.log(
      `starting search using config ${JSON.stringify(config, null, 4)}`,
    );
    running = true;
    get_duplicates(config).then((res) => {
      console.log(`res ${JSON.stringify(res, null, 4)}`);
      // res.forEach((entry) => {
      //   entry.hash = entry.hash.substring(6);
      // });
      //
      const newFiles = res.map((entry) => {
        let filesNew = entry.paths.map((afile) => {
          let afileNew: AFileRequest = {
            file_name: afile.file_name,
            file_size: afile.file_size,
            chrono_created: afile.chrono_created,
            path: afile.path,
            selected: false,
          };
          return afileNew;
        });
        let req: DuplicateFilesRequest = {
          hash: entry.hash,
          paths: filesNew,
          cnt_duplicates: entry.cnt_duplicates,
        };
        return req;
      });

      duplicates = newFiles;
      running = false;
    });
  };
</script>

<LoadingSpinner show={running} />

<div class="container">
  <div class="row">
    <div class="col-lg-12">
      <form>
        <div class="row mb-3">
          <label for="inputEmail3" class="col-sm-2 col-form-label"
            >Root folder:</label
          >
          <div class="col-sm-10">
            <input
              type="text"
              class="form-control"
              value={config.root_folder}
              id="inputEmail3"
            />
          </div>
        </div>

        <div class="row mb-3">
          <label for="inputPassword3" class="col-sm-2 col-form-label"
            >Target folder:</label
          >
          <div class="col-sm-10">
            <input
              type="text"
              class="form-control"
              id="inputPassword3"
              value={config.target_folder}
            />
          </div>
        </div>

        <div class="row mb-3">
          <label for="inputPassword3" class="col-sm-2 col-form-label"
            >Minimum filesize:</label
          >
          <div class="col-sm-10">
            <input
              type="number"
              class="form-control"
              id="inputPassword3"
              value={config.min_file_size}
            />
          </div>
        </div>

        <div class="row mb-3">
          {#if config.skip_folders.length === 0}
            <p>no folder to skip</p>
          {:else}
            <label for="inputPassword3" class="col-sm-2 col-form-label"
              >Skip folders:</label
            >
            <div class="col-sm-10">
              <table class="table">
                <tbody>
                  {#each config.skip_folders as folder}
                    <tr>
                      <td>
                        <input
                          type="text"
                          class="form-control"
                          id="inputPassword3"
                          value={folder}
                        />
                      </td>
                      <td
                        ><svg
                          xmlns="http://www.w3.org/2000/svg"
                          width="16"
                          height="16"
                          fill="currentColor"
                          class="bi bi-trash"
                          viewBox="0 0 16 16"
                        >
                          <path
                            d="M5.5 5.5A.5.5 0 0 1 6 6v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m2.5 0a.5.5 0 0 1 .5.5v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m3 .5a.5.5 0 0 0-1 0v6a.5.5 0 0 0 1 0z"
                          />
                          <path
                            d="M14.5 3a1 1 0 0 1-1 1H13v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V4h-.5a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1H6a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1h3.5a1 1 0 0 1 1 1zM4.118 4 4 4.059V13a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V4.059L11.882 4zM2.5 3h11V2h-11z"
                          />
                        </svg></td
                      >
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>

        <div class="row mb-3">
          {#if config.skip_filenames.length === 0}
            <p>no filenames to skip</p>
          {:else}
            <label for="inputPassword3" class="col-sm-2 col-form-label"
              >Skip filenames:</label
            >
            <div class="col-sm-10">
              <table class="table">
                <tbody>
                  {#each config.skip_filenames as folder}
                    <tr>
                      <td>
                        <input
                          type="text"
                          class="form-control"
                          id="inputPassword3"
                          value={folder}
                        /></td
                      >
                      <td
                        ><svg
                          xmlns="http://www.w3.org/2000/svg"
                          width="16"
                          height="16"
                          fill="currentColor"
                          class="bi bi-trash"
                          viewBox="0 0 16 16"
                        >
                          <path
                            d="M5.5 5.5A.5.5 0 0 1 6 6v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m2.5 0a.5.5 0 0 1 .5.5v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m3 .5a.5.5 0 0 0-1 0v6a.5.5 0 0 0 1 0z"
                          />
                          <path
                            d="M14.5 3a1 1 0 0 1-1 1H13v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V4h-.5a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1H6a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1h3.5a1 1 0 0 1 1 1zM4.118 4 4 4.059V13a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V4.059L11.882 4zM2.5 3h11V2h-11z"
                          />
                        </svg></td
                      >
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>

        <div class="row mb-3">
          {#if config.consider_extensions.length === 0}
            <p>no extensions to consider</p>
          {:else}
            <label for="inputPassword3" class="col-sm-2 col-form-label"
              >Consider extensions:</label
            >
            <div class="col-sm-10">
              <table class="table">
                <tbody>
                  {#each config.consider_extensions as folder}
                    <tr>
                      <td>
                        <input
                          type="text"
                          class="form-control"
                          id="inputPassword3"
                          value={folder}
                        /></td
                      >
                      <td
                        ><svg
                          xmlns="http://www.w3.org/2000/svg"
                          width="16"
                          height="16"
                          fill="currentColor"
                          class="bi bi-trash"
                          viewBox="0 0 16 16"
                        >
                          <path
                            d="M5.5 5.5A.5.5 0 0 1 6 6v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m2.5 0a.5.5 0 0 1 .5.5v6a.5.5 0 0 1-1 0V6a.5.5 0 0 1 .5-.5m3 .5a.5.5 0 0 0-1 0v6a.5.5 0 0 0 1 0z"
                          />
                          <path
                            d="M14.5 3a1 1 0 0 1-1 1H13v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V4h-.5a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1H6a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1h3.5a1 1 0 0 1 1 1zM4.118 4 4 4.059V13a1 1 0 0 0 1 1h6a1 1 0 0 0 1-1V4.059L11.882 4zM2.5 3h11V2h-11z"
                          />
                        </svg></td
                      >
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>

        <button type="submit" class="btn btn-primary" onclick={start_search}
          >Start searching duplicates</button
        >
      </form>
    </div>
  </div>
</div>

<div class="container-fluid">
  <div class="row">
    {#if has_data}
      <div class="col-lg-12">
        <h2>Duplicates</h2>
        <table class="table">
          <thead>
            <tr>
              <td>nr of duplicates</td>
              <td>Files</td>
            </tr>
          </thead>
          <tbody>
            {#each duplicates as duplicate}
              <tr>
                <td>{duplicate.cnt_duplicates}</td>
                <td>
                  {#each duplicate.paths as ffile}
                    <table class="table">
                      <tbody>
                        <tr>
                          <td><Path path={ffile.path} /></td>
                          <td>{ffile.chrono_created}</td>
                          <td><Number number={ffile.file_size} /></td>
                        </tr>
                      </tbody>
                    </table>
                  {/each}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>
</div>
