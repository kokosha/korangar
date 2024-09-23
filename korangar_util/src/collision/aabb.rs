use cgmath::{Array, ElementWise, EuclideanSpace, Matrix4, Point3, Vector3};
#[cfg(feature = "interface")]
use korangar_interface::elements::PrototypeElement;

use crate::collision::quadtree::{Insertable, Query};
use crate::collision::Sphere;
use crate::math::multiply_matrix4_and_vector3;

/// An axis aligned bounding box.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "interface", derive(PrototypeElement))]
pub struct AABB {
    min: Point3<f32>,
    max: Point3<f32>,
}

impl AABB {
    /// Create a new AABB from two points.
    pub fn new(p0: Point3<f32>, p1: Point3<f32>) -> Self {
        AABB {
            min: Point3::new(p0.x.min(p1.x), p0.y.min(p1.y), p0.z.min(p1.z)),
            max: Point3::new(p0.x.max(p1.x), p0.y.max(p1.y), p0.z.max(p1.z)),
        }
    }

    /// Calculates the axis aligned bounding box from a list of vertices.
    pub fn from_vertices<T>(vertex_positions: T) -> Self
    where
        T: IntoIterator<Item = Vector3<f32>>,
    {
        let mut min = Vector3::from_value(f32::MAX);
        let mut max = Vector3::from_value(-f32::MAX);

        for position in vertex_positions {
            min = min.zip(position, f32::min);
            max = max.zip(position, f32::max);
        }

        Self {
            min: Point3::from_vec(min),
            max: Point3::from_vec(max),
        }
    }

    /// Create an AABB from a center point and half-extents.
    pub fn from_center_and_size(center: Point3<f32>, size: Vector3<f32>) -> Self {
        let half_size = size.mul_element_wise(0.5);
        AABB {
            min: center - half_size,
            max: center + half_size,
        }
    }

    /// Creates the bounding box from an affine transformation matrix.
    pub fn from_transformation_matrix(transformation: Matrix4<f32>) -> AABB {
        // Define 4 corners of the unit cube that cover
        // all combinations of min/max per axis.
        let corners = [
            Vector3::new(-1.0, -1.0, -1.0),
            Vector3::new(-1.0, 1.0, 1.0),
            Vector3::new(1.0, -1.0, 1.0),
            Vector3::new(1.0, 1.0, -1.0),
        ];

        let transformed_corners = corners.map(|corner| multiply_matrix4_and_vector3(&transformation, corner));

        Self::from_vertices(transformed_corners)
    }

    /// Creates a point without a meaningful value.
    pub fn uninitialized() -> Self {
        let min: Point3<f32> = Point3::from_value(f32::MAX);
        let max: Point3<f32> = Point3::from_value(-f32::MAX);

        Self { min, max }
    }

    /// Get the minimum point of the AABB.
    pub fn min(&self) -> Point3<f32> {
        self.min
    }

    /// Get the maximum point of the AABB.
    pub fn max(&self) -> Point3<f32> {
        self.max
    }

    /// Get the center of the AABB.
    pub fn center(&self) -> Point3<f32> {
        (self.min + self.max.to_vec()) * 0.5
    }

    /// Get the size (dimensions) of the AABB.
    pub fn size(&self) -> Vector3<f32> {
        self.max - self.min
    }

    /// Check if a point is inside the AABB.
    pub fn contains_point(&self, point: Point3<f32>) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
            && point.z >= self.min.z
            && point.z <= self.max.z
    }

    /// Check if this AABB intersects with a sphere.
    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        sphere.intersects_aabb(self)
    }

    /// Check if this AABB intersects with another AABB.
    pub fn intersects_aabb(&self, other: &AABB) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }

    /// Expand the AABB to include a point.
    pub fn expand(&mut self, point: Point3<f32>) {
        self.min.x = self.min.x.min(point.x);
        self.min.y = self.min.y.min(point.y);
        self.min.z = self.min.z.min(point.z);
        self.max.x = self.max.x.max(point.x);
        self.max.y = self.max.y.max(point.y);
        self.max.z = self.max.z.max(point.z);
    }

    /// Merge this AABB with another AABB.
    pub fn merge(&self, other: &AABB) -> AABB {
        AABB {
            min: self.min.zip(other.min, f32::min),
            max: self.max.zip(other.max, f32::max),
        }
    }

    /// Extends the current AABB with another AABB.
    pub fn extend(&mut self, other: &Self) {
        self.min = self.min.zip(other.min, f32::min);
        self.max = self.max.zip(other.max, f32::max);
    }
}

impl Insertable for AABB {
    fn intersects_aabb(&self, aabb: &AABB) -> bool {
        self.intersects_aabb(aabb)
    }

    fn max_y(&self) -> f32 {
        self.max().y
    }

    fn min_y(&self) -> f32 {
        self.min.y
    }
}

impl Query<AABB> for AABB {
    fn intersects_aabb(&self, aabb: &AABB) -> bool {
        self.intersects_aabb(aabb)
    }

    fn intersects_object(&self, object: &AABB) -> bool {
        self.intersects_aabb(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let aabb = AABB::new(Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0));
        assert_eq!(aabb.min(), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb.max(), Point3::new(4.0, 5.0, 6.0));

        let aabb_reversed = AABB::new(Point3::new(4.0, 5.0, 6.0), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb_reversed.min(), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb_reversed.max(), Point3::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn test_from_center_and_size() {
        let center = Point3::new(0.0, 0.0, 0.0);
        let size = Vector3::new(2.0, 2.0, 2.0);
        let aabb = AABB::from_center_and_size(center, size);
        assert_eq!(aabb.min(), Point3::new(-1.0, -1.0, -1.0));
        assert_eq!(aabb.max(), Point3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn test_center() {
        let aabb = AABB::new(Point3::new(-1.0, -2.0, -3.0), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb.center(), Point3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_size() {
        let aabb = AABB::new(Point3::new(-1.0, -2.0, -3.0), Point3::new(1.0, 2.0, 3.0));
        assert_eq!(aabb.size(), Vector3::new(2.0, 4.0, 6.0));
    }

    #[test]
    fn test_contains_point() {
        let aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 2.0));
        assert!(aabb.contains_point(Point3::new(1.0, 1.0, 1.0)));
        assert!(aabb.contains_point(Point3::new(0.0, 0.0, 0.0)));
        assert!(aabb.contains_point(Point3::new(2.0, 2.0, 2.0)));
        assert!(!aabb.contains_point(Point3::new(-1.0, 1.0, 1.0)));
        assert!(!aabb.contains_point(Point3::new(3.0, 1.0, 1.0)));
    }

    #[test]
    fn test_intersects() {
        let aabb1 = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 2.0));
        let aabb2 = AABB::new(Point3::new(1.0, 1.0, 1.0), Point3::new(3.0, 3.0, 3.0));
        let aabb3 = AABB::new(Point3::new(3.0, 3.0, 3.0), Point3::new(4.0, 4.0, 4.0));
        assert!(aabb1.intersects_aabb(&aabb2));
        assert!(aabb2.intersects_aabb(&aabb1));
        assert!(!aabb1.intersects_aabb(&aabb3));
        assert!(aabb2.intersects_aabb(&aabb3));
    }

    #[test]
    fn test_expand() {
        let mut aabb = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        aabb.expand(Point3::new(2.0, -1.0, 0.5));
        assert_eq!(aabb.min(), Point3::new(0.0, -1.0, 0.0));
        assert_eq!(aabb.max(), Point3::new(2.0, 1.0, 1.0));
    }

    #[test]
    fn test_merge() {
        let aabb1 = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        let aabb2 = AABB::new(Point3::new(-1.0, -1.0, -1.0), Point3::new(2.0, 2.0, 2.0));
        let merged = aabb1.merge(&aabb2);
        assert_eq!(merged.min(), Point3::new(-1.0, -1.0, -1.0));
        assert_eq!(merged.max(), Point3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn test_aabb_with_negative_dimensions() {
        let aabb = AABB::new(Point3::new(2.0, 2.0, 2.0), Point3::new(1.0, 1.0, 1.0));
        assert_eq!(aabb.min(), Point3::new(1.0, 1.0, 1.0));
        assert_eq!(aabb.max(), Point3::new(2.0, 2.0, 2.0));
        assert_eq!(aabb.size(), Vector3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn test_aabb_intersects_edge_cases() {
        let aabb1 = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        let aabb2 = AABB::new(Point3::new(1.0, 1.0, 1.0), Point3::new(2.0, 2.0, 2.0));
        let aabb3 = AABB::new(Point3::new(1.0, 1.0, 1.0), Point3::new(1.0, 1.0, 1.0));
        assert!(aabb1.intersects_aabb(&aabb2));
        assert!(aabb1.intersects_aabb(&aabb3));
        assert!(aabb2.intersects_aabb(&aabb3));
    }

    #[test]
    fn test_from_vertices() {
        let vertices = vec![
            Vector3::new(1.0, 2.0, 3.0),
            Vector3::new(-1.0, 4.0, 0.0),
            Vector3::new(2.0, -2.0, 5.0),
        ];
        let aabb = AABB::from_vertices(vertices);
        assert_eq!(aabb.min(), Point3::new(-1.0, -2.0, 0.0));
        assert_eq!(aabb.max(), Point3::new(2.0, 4.0, 5.0));
    }

    #[test]
    fn test_from_transformation_matrix() {
        let translation = Vector3::new(1.0, 2.0, 3.0);
        let scale = Vector3::new(2.0, 2.0, 2.0);
        let transformation = Matrix4::from_translation(translation) * Matrix4::from_nonuniform_scale(scale.x, scale.y, scale.z);
        let aabb = AABB::from_transformation_matrix(transformation);
        assert_eq!(aabb.min(), Point3::new(-1.0, 0.0, 1.0));
        assert_eq!(aabb.max(), Point3::new(3.0, 4.0, 5.0));
    }

    #[test]
    fn test_uninitialized() {
        let aabb = AABB::uninitialized();
        assert_eq!(aabb.min(), Point3::new(f32::MAX, f32::MAX, f32::MAX));
        assert_eq!(aabb.max(), Point3::new(-f32::MAX, -f32::MAX, -f32::MAX));
    }

    #[test]
    fn test_extend() {
        let mut aabb1 = AABB::new(Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0));
        let aabb2 = AABB::new(Point3::new(-1.0, -1.0, -1.0), Point3::new(2.0, 2.0, 2.0));
        aabb1.extend(&aabb2);
        assert_eq!(aabb1.min(), Point3::new(-1.0, -1.0, -1.0));
        assert_eq!(aabb1.max(), Point3::new(2.0, 2.0, 2.0));
    }

    #[test]
    fn test_intersects_sphere() {
        let aabb = AABB::new(Point3::new(-1.0, -1.0, -1.0), Point3::new(1.0, 1.0, 1.0));
        let sphere_inside = Sphere::new(Point3::new(0.0, 0.0, 0.0), 0.5);
        let sphere_intersecting = Sphere::new(Point3::new(0.0, 0.0, 0.0), 1.5);
        let sphere_outside = Sphere::new(Point3::new(3.0, 3.0, 3.0), 0.5);
        assert!(aabb.intersects_sphere(&sphere_inside));
        assert!(aabb.intersects_sphere(&sphere_intersecting));
        assert!(!aabb.intersects_sphere(&sphere_outside));
    }
}
