import { defineConfig } from '@playwright/test'

// The page's modal checks in WebKit, the engine of the app's own window (`npm run test:ui:webkit`):
// the CI runner installs Chromium only, so this is run by hand, after `npx playwright install webkit`.
export default defineConfig({
  testDir: 'tests',
  use: { browserName: 'webkit', deviceScaleFactor: 2 },
})
