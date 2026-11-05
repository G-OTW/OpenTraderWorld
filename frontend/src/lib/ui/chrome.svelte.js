// Shell chrome a page can fold away. A page that sets a flag clears it when it unmounts,
// so leaving the page always brings the chrome back.
export const chrome = $state({ hideTopbar: false });
