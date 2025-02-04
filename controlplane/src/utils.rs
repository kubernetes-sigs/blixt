use crate::traits::HasConditions;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

// Sets the provided condition on any Gateway API object so log as it implements
// the HasConditions trait.
//
// The condition on the object is only updated
// if the new condition has a different status (except for the observed generation which is always
// updated).
pub fn set_condition<T: HasConditions>(obj: &mut T, new_cond: metav1::Condition) {
    if let Some(ref mut conditions) = obj.get_conditions_mut() {
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
