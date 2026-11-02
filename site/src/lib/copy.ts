/** Wires every `[data-copy]` button to copy its text to the clipboard. */
export function wireCopyButtons() {
  for (const button of document.querySelectorAll<HTMLButtonElement>('[data-copy]')) {
    const label = button.textContent
    button.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(button.dataset.copy ?? '')
        button.textContent = 'Copied'
      } catch {
        button.textContent = 'Select and copy'
      }
      setTimeout(() => (button.textContent = label), 1600)
    })
  }
}
