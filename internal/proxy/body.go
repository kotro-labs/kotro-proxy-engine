package proxy

import (
	"errors"
	"io"
	"net/http"
)

func readLimitedBody(w http.ResponseWriter, r *http.Request, maxBytes int64) ([]byte, error) {
	if maxBytes <= 0 {
		maxBytes = 10 << 20
	}

	r.Body = http.MaxBytesReader(w, r.Body, maxBytes)
	defer r.Body.Close()

	body, err := io.ReadAll(r.Body)
	if err != nil {
		var maxErr *http.MaxBytesError
		if errors.As(err, &maxErr) {
			http.Error(w, "request body too large", http.StatusRequestEntityTooLarge)
			return nil, err
		}
		http.Error(w, "read body", http.StatusBadRequest)
		return nil, err
	}
	return body, nil
}
