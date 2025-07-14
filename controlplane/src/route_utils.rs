/*
Copyright 2024 The Kubernetes Authors.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

use crate::traits::HasConditions;

// Sets the provided condition on any Gateway API object so log as it implements
// the HasConditions trait.
//
// The condition on the object is only updated
// if the new condition has a different status (except for the observed generation which is always
// updated).
pub fn set_condition<T: HasConditions>(obj: &mut T, new_cond: metav1::Condition) {
    if let Some(conditions) = obj.get_conditions_mut() {
        for condition in conditions.iter_mut() {
            if condition.type_ == new_cond.type_ {
                if condition.status == new_cond.status {
                    // always update the observed generation
                    condition.observed_generation = new_cond.observed_generation;
                    return;
                }
                *condition = new_cond;
                return;
            }
        }
        conditions.push(new_cond);
    } else {
        obj.get_conditions_mut().replace(vec![new_cond]);
    }
}
