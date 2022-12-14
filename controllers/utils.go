package controllers

import (
	"context"

	"sigs.k8s.io/controller-runtime/pkg/client"
)

const (
	// DataPlaneFinalizer is the finalizer which indicates that an object needs to
	// have its configuration removed from the dataplane before it can be deleted.
	DataPlaneFinalizer = "blixt/dataplane-configuration"
)

func isDataPlaneFinalizerSet(obj client.Object) bool {
	for _, finalizer := range obj.GetFinalizers() {
		if finalizer == DataPlaneFinalizer {
			return true
		}
	}
	return false
}

func setDataPlaneFinalizer(ctx context.Context, c client.Client, obj client.Object) error {
	finalizers := obj.GetFinalizers()
	obj.SetFinalizers(append(finalizers, DataPlaneFinalizer))
	return c.Update(ctx, obj)
}
