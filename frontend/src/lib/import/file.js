/** Read a picked file as a base64 `data:` URI.
 *
 * The file never leaves the browser between steps: every analyze/commit call ships it
 * again, so there is no server-side upload state to expire, resume or clean up. */
export function fileToBase64(file) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error('could not read the file'));
    reader.readAsDataURL(file);
  });
}
