export interface AFile {
  file_name: string;
  file_size: number;
  created: Date;
  chrono_created: Date;
  path: string;
}

export interface DuplicateFiles {
  hash: string;
  paths: Array<AFile>;
  cnt_duplicates: number;
}

export interface Config {
  root_folder: string;
  target_folder: string;
  skip_folders: Array<string>;
  skip_filenames: Array<string>;
  min_file_size: number;
  consider_extensions: Array<string>;
}

export type PropsConfig = {
  config: Config;
};


export interface AFileRequest {
  file_name: string;
  file_size: number;
  chrono_created: Date;
  path: string;
  selected: boolean;
}

export interface DuplicateFilesRequest {
  hash: string;
  paths: Array<AFileRequest>;
  cnt_duplicates: number;
}

