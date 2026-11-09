/**
 * A click gives the control it lands on the keyboard, in every engine. Chromium and WebView2
 * do it of themselves; WebKit, the engine of the app's own window on a Mac, leaves a button, a
 * checkbox or a radio button unfocused when a click lands on it (the keyboard goes to nothing).
 * Every rule of this page about the keyboard is the first's: a dialog gives it back to the
 * control that opened it as it closes (the browser's own return goes to what had the keyboard
 * as the dialog opened, and in WebKit that was nothing), and a menu leaves it to a control the
 * click went to. So the page does what Chromium does, and these rules hold in WebKit as they do
 * there.
 *
 * Two clicks give nothing. One a script makes (`click()`) or the keyboard makes (Enter on a
 * field submitting its form) has no click count: the keyboard is where it was. And a menu's
 * items are the menu's own: it shuts as one is chosen and gives the keyboard where its rule
 * says, and an item that has the keyboard as a dialog opens would be that dialog's opener, gone
 * when it closes: measured in WebKit, the menu's own return of the keyboard to the project then
 * does nothing when the dialog is closed with its Save button, and the keyboard is left on nothing.
 */
const CONTROLS = 'button, input[type="checkbox"], input[type="radio"]'

/**
 * Listens for clicks on the whole page, before any control's own handler runs: the control has
 * the keyboard when its handler opens a dialog, which is what the dialog gives it back to.
 */
export function focusWhatIsClicked() {
  document.addEventListener(
    'click',
    (event) => {
      if (event.detail === 0) return
      const control = event.target.closest?.(CONTROLS)
      if (!control || control === document.activeElement || control.closest('[role="menu"]')) return
      control.focus({ preventScroll: true })
    },
    true,
  )
}
