import { BinariesMockData } from '../../utils/mocks/binaryHandlers';
import { UpdaterMockData } from '../../utils/mocks/updateHandlers';
import { StrongholdMockData } from '../../utils/mocks/strongholdHandlers';

import { AppearanceSettings, UpdateSettings } from '../../../src/tauri/types/config';

export type MockData = {
  settings?: {
    appearance?: Partial<AppearanceSettings>;
    update?: Partial<UpdateSettings>;
  };
  binaries?: BinariesMockData;
  updater?: UpdaterMockData;
  stronghold?: StrongholdMockData;
};
