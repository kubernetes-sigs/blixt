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

func setDataPlaneFinalizer(ctx context.Context, c client.Client, obj client.Object) error {
	finalizers := obj.GetFinalizers()
	obj.SetFinalizers(append(finalizers, DataPlaneFinalizer))
	return c.Update(ctx, obj)
}
