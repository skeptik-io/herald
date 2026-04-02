package herald

import "fmt"

// HeraldError is an error returned by the Herald API.
type HeraldError struct {
	Code    string
	Message string
	Status  int
}

func (e *HeraldError) Error() string {
	return fmt.Sprintf("%s: %s", e.Code, e.Message)
}
