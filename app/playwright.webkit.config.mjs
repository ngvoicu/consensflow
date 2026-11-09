import { defineConfig } from '@playwright/test'

// The page's whole suite in WebKit, the engine of the app's own window on a Mac (`npm run test:ui:webkit`,
// the six specs `npm run test:ui` runs): the CI runner installs Chromium only, so this is run by hand, after
// `npx playwright install webkit`.
export default defineConfig({
  testDir: 'tests',
  use: { browserName: 'webkit', deviceScaleFactor: 2 },
})
