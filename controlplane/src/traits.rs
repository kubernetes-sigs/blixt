use gateway_api::apis::standard::{gatewayclasses::GatewayClass, gateways::Gateway};
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

pub trait HasConditions {
    fn get_conditions_mut(&mut self) -> &mut Option<Vec<metav1::Condition>>;
}

impl HasConditions for Gateway {
    fn get_conditions_mut(&mut self) -> &mut Option<Vec<metav1::Condition>> {
        &mut self.status.as_mut().unwrap().conditions
    }
}

impl HasConditions for GatewayClass {
    fn get_conditions_mut(&mut self) -> &mut Option<Vec<metav1::Condition>> {
        &mut self.status.as_mut().unwrap().conditions
    }
}
